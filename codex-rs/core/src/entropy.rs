//! Extension points for deterministic execution.
//!
//! This module provides traits for randomness and time that can be replaced
//! with deterministic implementations for replay/workflow scenarios.

use std::fmt::Debug;
use std::ops::Range;
use std::sync::Arc;
use std::time::Instant;
use std::time::SystemTime;

/// Source of randomness (UUIDs, random numbers).
pub trait RandomSource: Send + Sync + Debug {
    /// Generate a UUID string.
    fn uuid(&self) -> String;
    /// Generate a random f64 in [0, 1).
    fn f64(&self) -> f64;
    /// Generate a random u64.
    fn u64(&self) -> u64;
    /// Generate a random f64 in the given range.
    fn f64_range(&self, range: Range<f64>) -> f64;
}

/// Source of time.
pub trait Clock: Send + Sync + Debug {
    /// Returns a monotonic instant (for measuring durations).
    fn now(&self) -> Instant;
    /// Returns wall-clock time.
    fn wall_time(&self) -> SystemTime;
    /// Returns milliseconds since Unix epoch.
    fn unix_millis(&self) -> u64;
}

/// Combined entropy providers for injection.
#[derive(Clone)]
pub struct EntropyProviders {
    pub random: Arc<dyn RandomSource>,
    pub clock: Arc<dyn Clock>,
}

// --- Default Implementations ---

/// System-based random source using uuid and rand crates.
#[derive(Debug, Default)]
pub struct SystemRandomSource;

impl RandomSource for SystemRandomSource {
    fn uuid(&self) -> String {
        uuid::Uuid::new_v4().to_string()
    }

    fn f64(&self) -> f64 {
        rand::random()
    }

    fn u64(&self) -> u64 {
        rand::random()
    }

    fn f64_range(&self, range: Range<f64>) -> f64 {
        rand::Rng::random_range(&mut rand::rng(), range)
    }
}

/// System-based clock using std::time.
#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn wall_time(&self) -> SystemTime {
        SystemTime::now()
    }

    fn unix_millis(&self) -> u64 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

impl Default for EntropyProviders {
    fn default() -> Self {
        Self {
            random: Arc::new(SystemRandomSource),
            clock: Arc::new(SystemClock),
        }
    }
}

impl Debug for EntropyProviders {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EntropyProviders")
            .field("random", &self.random)
            .field("clock", &self.clock)
            .finish()
    }
}

tokio::task_local! {
    /// Task-local entropy providers for the agentic loop.
    ///
    /// Scoped at the entry point of the loop so that all code within has access
    /// to the configured randomness and clock sources. The in-process path uses
    /// `SystemRandomSource`/`SystemClock` (the defaults). A Temporal workflow
    /// provides deterministic implementations backed by workflow context.
    pub static ENTROPY: EntropyProviders;
}

/// Helper to access the task-local random UUID generator.
///
/// Falls back to `Uuid::new_v4()` when called outside a scoped context (e.g.,
/// in tests or code paths that have not been migrated yet).
pub fn entropy_uuid() -> String {
    ENTROPY
        .try_with(|e| e.random.uuid())
        .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string())
}

/// Helper to access the task-local monotonic clock.
///
/// Falls back to `Instant::now()` when called outside a scoped context.
pub fn entropy_now() -> Instant {
    ENTROPY
        .try_with(|e| e.clock.now())
        .unwrap_or_else(|_| Instant::now())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn system_random_source_generates_unique_uuids() {
        let source = SystemRandomSource;
        let uuids: HashSet<_> = (0..100).map(|_| source.uuid()).collect();
        assert_eq!(uuids.len(), 100, "UUIDs should be unique");
    }

    #[test]
    fn system_random_source_f64_in_range() {
        let source = SystemRandomSource;
        for _ in 0..100 {
            let val = source.f64();
            assert!((0.0..1.0).contains(&val), "f64 should be in [0, 1)");
        }
    }

    #[test]
    fn system_random_source_f64_range_respects_bounds() {
        let source = SystemRandomSource;
        for _ in 0..100 {
            let val = source.f64_range(0.9..1.1);
            assert!(
                (0.9..1.1).contains(&val),
                "f64_range should respect bounds, got {val}"
            );
        }
    }

    #[test]
    fn system_clock_now_is_monotonic() {
        let clock = SystemClock;
        let t1 = clock.now();
        let t2 = clock.now();
        assert!(t2 >= t1, "Clock should be monotonic");
    }

    #[test]
    fn system_clock_unix_millis_is_reasonable() {
        let clock = SystemClock;
        let millis = clock.unix_millis();
        // Should be after 2020 (1577836800000) and before 2100
        assert!(millis > 1577836800000, "unix_millis should be after 2020");
        assert!(millis < 4102444800000, "unix_millis should be before 2100");
    }

    #[test]
    fn entropy_providers_default_uses_system_sources() {
        let providers = EntropyProviders::default();
        // Should not panic and should produce valid output
        let uuid = providers.random.uuid();
        assert!(!uuid.is_empty());
        let _ = providers.clock.now();
    }

    // Test helper for deterministic testing
    #[derive(Debug)]
    struct MockRandomSource {
        uuid_sequence: std::sync::Mutex<Vec<String>>,
        f64_value: f64,
    }

    impl MockRandomSource {
        fn new(uuids: Vec<&str>, f64_value: f64) -> Self {
            Self {
                uuid_sequence: std::sync::Mutex::new(
                    uuids.into_iter().map(String::from).rev().collect(),
                ),
                f64_value,
            }
        }
    }

    impl RandomSource for MockRandomSource {
        fn uuid(&self) -> String {
            self.uuid_sequence
                .lock()
                .unwrap()
                .pop()
                .unwrap_or_else(|| "fallback-uuid".to_string())
        }

        fn f64(&self) -> f64 {
            self.f64_value
        }

        fn u64(&self) -> u64 {
            self.f64_value as u64
        }

        fn f64_range(&self, range: Range<f64>) -> f64 {
            range.start + self.f64_value * (range.end - range.start)
        }
    }

    #[test]
    fn mock_random_source_returns_predictable_values() {
        let mock = MockRandomSource::new(vec!["uuid-1", "uuid-2"], 0.5);
        assert_eq!(mock.uuid(), "uuid-1");
        assert_eq!(mock.uuid(), "uuid-2");
        assert_eq!(mock.f64(), 0.5);
        assert_eq!(mock.f64_range(0.0..10.0), 5.0);
    }
}
