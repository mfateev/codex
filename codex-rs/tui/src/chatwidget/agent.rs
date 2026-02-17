use std::sync::Arc;

use codex_core::AgentSession;
use codex_core::CodexThread;
use codex_core::NewThread;
use codex_core::ThreadManager;
use codex_core::config::Config;
use codex_core::protocol::Event;
use codex_core::protocol::EventMsg;
use codex_core::protocol::Op;
use codex_core::protocol::SessionConfiguredEvent;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::mpsc::unbounded_channel;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;

/// Wire any [`AgentSession`] implementation into the TUI event loop.
///
/// Sends the provided `SessionConfiguredEvent` immediately, then spawns the
/// op-forwarding and event-listening loops, returning the `UnboundedSender<Op>`
/// that the UI uses to submit operations.
///
/// This is the public entry point for alternative backends (e.g. a Temporal
/// workflow adapter).
pub(crate) fn wire_session(
    session: Arc<dyn AgentSession>,
    session_configured: SessionConfiguredEvent,
    app_event_tx: AppEventSender,
) -> UnboundedSender<Op> {
    let (codex_op_tx, codex_op_rx) = unbounded_channel::<Op>();

    tokio::spawn(run_session_loops(
        session,
        session_configured,
        codex_op_rx,
        app_event_tx,
    ));

    codex_op_tx
}

/// Shared forwarding logic: sends the `SessionConfigured` event, then runs the
/// op-submission and event-listening loops until the session ends.
async fn run_session_loops(
    session: Arc<dyn AgentSession>,
    session_configured: SessionConfiguredEvent,
    codex_op_rx: UnboundedReceiver<Op>,
    app_event_tx: AppEventSender,
) {
    // Forward the captured `SessionConfigured` event so it can be rendered in the UI.
    let ev = Event {
        // The `id` does not matter for rendering, so we can use a fake value.
        id: "".to_string(),
        msg: EventMsg::SessionConfigured(session_configured),
    };
    app_event_tx.send(AppEvent::CodexEvent(ev));

    spawn_op_submit_loop(session.clone(), codex_op_rx);

    while let Ok(event) = session.next_event().await {
        let is_shutdown_complete = matches!(event.msg, EventMsg::ShutdownComplete);
        app_event_tx.send(AppEvent::CodexEvent(event));
        if is_shutdown_complete {
            // ShutdownComplete is terminal for a thread; drop this receiver task so
            // the Arc can be released and thread resources can clean up.
            break;
        }
    }
}

/// Spawn a task that reads Ops from the channel and submits them to the session.
fn spawn_op_submit_loop(
    session: Arc<dyn AgentSession>,
    mut codex_op_rx: UnboundedReceiver<Op>,
) {
    tokio::spawn(async move {
        while let Some(op) = codex_op_rx.recv().await {
            if let Err(e) = session.submit(op).await {
                tracing::error!("failed to submit op: {e}");
            }
        }
    });
}

/// Spawn the agent bootstrapper and op forwarding loop, returning the
/// `UnboundedSender<Op>` used by the UI to submit operations.
pub(crate) fn spawn_agent(
    config: Config,
    app_event_tx: AppEventSender,
    server: Arc<ThreadManager>,
) -> UnboundedSender<Op> {
    let (codex_op_tx, codex_op_rx) = unbounded_channel::<Op>();

    tokio::spawn(async move {
        let NewThread {
            thread,
            session_configured,
            ..
        } = match server.start_thread(config).await {
            Ok(v) => v,
            Err(err) => {
                let message = format!("Failed to initialize codex: {err}");
                tracing::error!("{message}");
                app_event_tx.send(AppEvent::CodexEvent(Event {
                    id: "".to_string(),
                    msg: EventMsg::Error(err.to_error_event(None)),
                }));
                app_event_tx.send(AppEvent::FatalExitRequest(message));
                tracing::error!("failed to initialize codex: {err}");
                return;
            }
        };

        run_session_loops(
            thread as Arc<dyn AgentSession>,
            session_configured,
            codex_op_rx,
            app_event_tx,
        )
        .await;
    });

    codex_op_tx
}

/// Spawn agent loops for an existing thread (e.g., a forked thread).
/// Sends the provided `SessionConfiguredEvent` immediately, then forwards subsequent
/// events and accepts Ops for submission.
pub(crate) fn spawn_agent_from_existing(
    thread: Arc<CodexThread>,
    session_configured: SessionConfiguredEvent,
    app_event_tx: AppEventSender,
) -> UnboundedSender<Op> {
    wire_session(thread as Arc<dyn AgentSession>, session_configured, app_event_tx)
}

/// Spawn an op-forwarding loop for an existing thread without subscribing to events.
pub(crate) fn spawn_op_forwarder(thread: std::sync::Arc<CodexThread>) -> UnboundedSender<Op> {
    let (codex_op_tx, mut codex_op_rx) = unbounded_channel::<Op>();

    tokio::spawn(async move {
        while let Some(op) = codex_op_rx.recv().await {
            if let Err(e) = thread.submit(op).await {
                tracing::error!("failed to submit op: {e}");
            }
        }
    });

    codex_op_tx
}
