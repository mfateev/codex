//! Pluggable I/O backends for the agentic loop.
//!
//! These traits decouple the core loop from specific I/O mechanisms (channels,
//! files), allowing alternative implementations for workflow engines like
//! Temporal that need buffer-backed event delivery and in-memory persistence.

use std::sync::Arc;

use crate::RolloutRecorder;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::RolloutItem;
use tokio::sync::Mutex;
use tracing::{debug, error, warn};

// ---------------------------------------------------------------------------
// EventSink
// ---------------------------------------------------------------------------

/// Delivers events from the agentic loop to consumers (TUI, app-server, etc.).
///
/// The default in-process implementation (`ChannelEventSink`) wraps an
/// `async_channel::Sender<Event>`. A Temporal workflow would provide a
/// `BufferEventSink` that appends to workflow state queried by the TUI.
#[async_trait::async_trait]
pub trait EventSink: Send + Sync {
    /// Deliver an event to the consumer.
    async fn emit_event(&self, event: Event);
}

/// Default in-process event sink backed by an async channel.
pub struct ChannelEventSink {
    tx: async_channel::Sender<Event>,
}

impl ChannelEventSink {
    pub fn new(tx: async_channel::Sender<Event>) -> Self {
        Self { tx }
    }
}

#[async_trait::async_trait]
impl EventSink for ChannelEventSink {
    async fn emit_event(&self, event: Event) {
        if let Err(e) = self.tx.send(event).await {
            debug!("dropping event because channel is closed: {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// StorageBackend
// ---------------------------------------------------------------------------

/// Persists rollout items so a session can be replayed or inspected later.
///
/// The default in-process implementation (`RolloutFileStorage`) wraps the
/// existing `RolloutRecorder` (JSONL files on disk). A Temporal workflow would
/// provide an `InMemoryStorage` backed by durable workflow state.
#[async_trait::async_trait]
pub trait StorageBackend: Send + Sync {
    /// Persist one or more rollout items.
    async fn save(&self, items: &[RolloutItem]);

    /// Flush any buffered writes to durable storage.
    ///
    /// The default implementation is a no-op, suitable for backends that write
    /// synchronously or have no concept of buffering.
    async fn flush(&self) {}
}

/// Default in-process storage backed by the existing `RolloutRecorder`.
pub struct RolloutFileStorage {
    rollout: Arc<Mutex<Option<RolloutRecorder>>>,
}

impl RolloutFileStorage {
    pub fn new(rollout: Arc<Mutex<Option<RolloutRecorder>>>) -> Self {
        Self { rollout }
    }
}

#[async_trait::async_trait]
impl StorageBackend for RolloutFileStorage {
    async fn save(&self, items: &[RolloutItem]) {
        let recorder = {
            let guard = self.rollout.lock().await;
            guard.clone()
        };
        if let Some(rec) = recorder
            && let Err(e) = rec.record_items(items).await
        {
            error!("failed to record rollout items: {e:#}");
        }
    }

    async fn flush(&self) {
        let recorder = {
            let guard = self.rollout.lock().await;
            guard.clone()
        };
        if let Some(rec) = recorder
            && let Err(e) = rec.flush().await
        {
            warn!("failed to flush rollout recorder: {e}");
        }
    }
}
