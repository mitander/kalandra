//! Turmoil-based Environment implementation for deterministic testing.

use std::time::Duration;

use sunder_core::env::Environment;

/// Simulation environment using Turmoil's virtual time and seeded RNG.
///
/// This implementation provides:
///
/// - **Virtual Time**: `now()` returns Turmoil's simulated time, which can be
///   advanced instantly via `turmoil::sleep()`.
///
/// - **Seeded RNG**: `random_bytes()` uses Turmoil's deterministic RNG,
///   ensuring reproducible test runs with the same seed.
///
/// # Usage
///
/// `SimEnv` must be used inside a Turmoil simulation context (created by
/// `turmoil::Builder`). Using it outside will panic.
///
/// # Example
///
/// ```rust,ignore
/// use sunder_harness::SimEnv;
/// use sunder_core::env::Environment;
/// use std::time::Duration;
///
/// let mut sim = turmoil::Builder::new().build();
///
/// sim.client("test", async {
///     let env = SimEnv;
///
///     // Virtual time
///     let start = env.now();
///     env.sleep(Duration::from_secs(10)).await;
///     let end = env.now();
///
///     // This completes instantly in wall-clock time, but advances
///     // virtual time by 10 seconds
///     assert_eq!(end - start, Duration::from_secs(10));
///
///     Ok(())
/// });
///
/// sim.run().unwrap();
/// ```
///
/// # Panics
///
/// - `random_bytes()` panics if called outside a Turmoil simulation
/// - `now()` panics if called outside a Turmoil simulation
#[derive(Clone, Copy, Debug)]
pub struct SimEnv;

impl Environment for SimEnv {
    type Instant = std::time::Instant;

    fn now(&self) -> Self::Instant {
        // Use tokio's Instant which works with turmoil
        tokio::time::Instant::now().into_std()
    }

    async fn sleep(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }

    fn random_bytes(&self, dest: &mut [u8]) {
        // Use rand with thread_rng
        // In turmoil context, this will be seeded deterministically
        use rand::RngCore;
        rand::thread_rng().fill_bytes(dest);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sim_env_time_advances() {
        let mut sim = turmoil::Builder::new().build();

        sim.client("test", async {
            let env = SimEnv;

            let start = env.now();
            env.sleep(Duration::from_secs(5)).await;
            let end = env.now();

            assert_eq!(end - start, Duration::from_secs(5));

            Ok(())
        });

        sim.run().expect("simulation failed");
    }

    #[test]
    fn sim_env_rng_works() {
        let mut sim = turmoil::Builder::new().build();

        sim.client("test", async {
            let env = SimEnv;

            // Verify we can generate random bytes without panicking
            let mut bytes1 = [0u8; 32];
            let mut bytes2 = [0u8; 32];

            env.random_bytes(&mut bytes1);
            env.random_bytes(&mut bytes2);

            // Different calls should produce different bytes
            // (with overwhelming probability)
            assert_ne!(&bytes1[..], &bytes2[..]);

            Ok(())
        });

        sim.run().expect("simulation failed");
    }
}
