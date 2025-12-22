//! Broadcast policy configuration for server I/O.
//!
//! Defines how the server handles broadcast failures when sending frames
//! to multiple recipients.

/// Policy for handling broadcast send failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BroadcastPolicy {
    /// Log failure and continue to next recipient.
    /// Suitable for simulation where we want to test failure scenarios.
    #[default]
    BestEffort,

    /// Retry failed sends with exponential backoff.
    /// Suitable for production where delivery matters.
    Retry {
        /// Maximum number of retry attempts
        max_attempts: u32,
        /// Initial backoff duration in milliseconds
        initial_backoff_ms: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broadcast_policy_default() {
        let policy = BroadcastPolicy::default();
        assert_eq!(policy, BroadcastPolicy::BestEffort);
    }

    #[test]
    fn broadcast_policy_retry() {
        let policy = BroadcastPolicy::Retry { max_attempts: 3, initial_backoff_ms: 100 };
        match policy {
            BroadcastPolicy::Retry { max_attempts, initial_backoff_ms } => {
                assert_eq!(max_attempts, 3);
                assert_eq!(initial_backoff_ms, 100);
            },
            _ => panic!("expected Retry policy"),
        }
    }
}
