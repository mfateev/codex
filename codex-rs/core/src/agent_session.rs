use crate::error::Result as CodexResult;
use crate::protocol::Event;
use crate::protocol::Op;

/// Minimal contract for a backend that can accept operations and produce events.
///
/// The default implementation is [`crate::CodexThread`] (in-process agent loop).
/// Alternative backends (e.g. a Temporal workflow) can implement this trait and
/// be wired into the TUI via [`codex_tui::chatwidget::wire_session`].
#[async_trait::async_trait]
pub trait AgentSession: Send + Sync + 'static {
    /// Submit an operation and return the turn/event id.
    async fn submit(&self, op: Op) -> CodexResult<String>;

    /// Block until the next event is available.
    async fn next_event(&self) -> CodexResult<Event>;
}
