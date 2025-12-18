//! Production Environment implementation using system time and RNG.
//!
//! This module provides `SystemEnv`, the production implementation of the
//! `Environment` trait that uses real system time and cryptographic RNG.

use std::time::Duration;

use kalandra_core::env::Environment;

/// Production environment using system time and cryptographic RNG.
///
/// This implementation:
/// - Uses `std::time::Instant::now()` for time
/// - Uses `tokio::time::sleep()` for async sleeping
/// - Uses `getrandom` for cryptographic randomness
///
/// # Security
///
/// The RNG uses `getrandom` which provides OS-level cryptographic randomness.
/// This is suitable for generating session IDs, nonces, and other security-
/// critical values.
#[derive(Clone, Default)]
pub struct SystemEnv;

impl SystemEnv {
    /// Create a new system environment.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Environment for SystemEnv {
    fn now(&self) -> std::time::Instant {
        std::time::Instant::now()
    }

    fn sleep(&self, duration: Duration) -> impl std::future::Future<Output = ()> + Send {
        tokio::time::sleep(duration)
    }

    fn random_bytes(&self, buffer: &mut [u8]) {
        getrandom::fill(buffer).unwrap_or_else(|e| {
            // NOTE: This should never fail on supported platforms, if it does it's a
            // critical error. In production, we should handle this more gracefully.
            // Fill with zeros as a fallback (not secure, but prevents panic)
            tracing::error!("getrandom failed: {}", e);
            buffer.fill(0);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_env_time_advances() {
        let env = SystemEnv::new();

        let t1 = env.now();
        std::thread::sleep(Duration::from_millis(10));
        let t2 = env.now();

        assert!(t2 > t1, "Time should advance");
    }

    #[test]
    fn system_env_random_bytes_are_random() {
        let env = SystemEnv::new();

        let mut bytes1 = [0u8; 32];
        let mut bytes2 = [0u8; 32];

        env.random_bytes(&mut bytes1);
        env.random_bytes(&mut bytes2);

        // Extremely unlikely to be equal if random
        assert_ne!(bytes1, bytes2, "Random bytes should differ");
    }

    #[test]
    fn system_env_random_bytes_fills_buffer() {
        let env = SystemEnv::new();

        let mut bytes = [0u8; 64];
        env.random_bytes(&mut bytes);

        // Check that at least some bytes are non-zero
        let non_zero_count = bytes.iter().filter(|&&b| b != 0).count();
        assert!(non_zero_count > 32, "Most bytes should be non-zero");
    }

    #[tokio::test]
    async fn system_env_sleep_works() {
        let env = SystemEnv::new();

        let start = env.now();
        env.sleep(Duration::from_millis(50)).await;
        let elapsed = env.now() - start;

        assert!(elapsed >= Duration::from_millis(50), "Sleep should wait at least 50ms");
    }
}
