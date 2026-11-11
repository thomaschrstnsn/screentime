//! The ADJUSTMENTS stream, which the collector creates.

use screentime_contract::{ADJUSTMENTS_STREAM, subject};

/// The JetStream stream recording `time.adjusted` events, to query by hand
/// (`nats stream view ADJUSTMENTS`).
pub fn adjustments_stream() -> async_nats::jetstream::stream::Config {
    use async_nats::jetstream::stream::{Config, DiscardPolicy, RetentionPolicy, StorageType};
    Config {
        name: ADJUSTMENTS_STREAM.into(),
        // Only `time.adjusted`: matching `time.adjust.>` would make JetStream answer
        // adjustment requests itself.
        subjects: vec![subject::adjusted("*", "*")],
        retention: RetentionPolicy::Limits,
        storage: StorageType::File,
        max_age: std::time::Duration::from_secs(365 * 24 * 60 * 60),
        discard: DiscardPolicy::Old,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether NATS subject `pattern` matches the concrete `subject`.
    fn matches(pattern: &str, subject: &str) -> bool {
        let mut pattern = pattern.split('.');
        let mut subject = subject.split('.');
        loop {
            match (pattern.next(), subject.next()) {
                (Some(">"), Some(_)) => return true,
                (Some("*"), Some(_)) => {}
                (Some(p), Some(s)) if p == s => {}
                (None, None) => return true,
                _ => return false,
            }
        }
    }

    #[test]
    fn adjustments_stream_captures_events_but_never_requests() {
        // A stream matching a request subject would answer it with a PubAck, which web
        // can't tell from a real reply.
        let subjects = adjustments_stream().subjects;
        let captures = |subject| subjects.iter().any(|p| matches(p, subject));

        assert!(captures("time.adjusted.test-vm.junior"));
        assert!(!captures("time.adjust.test-vm.junior"));
        assert!(!captures("time.obs.test-vm.junior"));
    }
}
