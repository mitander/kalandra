//! Environment abstraction for deterministic testing.
//!
//! The `Environment` trait decouples protocol logic from system resources
//! (time, randomness, network I/O). This enables:
//!
//! - Deterministic Simulation: Turmoil provides a virtual clock and seeded RNG,
//!   allowing perfect bug reproduction.
//!
//! - Production Runtime: Tokio/Quinn implementations use real system resources
//!   without any code changes to the protocol logic.
//!
//! # Design Philosophy: "The Hollow Shell"
//!
//! Protocol state machines in `kalandra-core` are pure logic. They:
//!
//! - MUST NOT call `std::time::Instant::now()` or `tokio::time::sleep()`
//! - MUST NOT use `rand::thread_rng()` or system entropy directly
//! - MUST accept an `Environment` parameter for all side effects
//!
//! The environment is implemented twice:
//!
//! 1. `SimEnv` (kalandra-harness): Uses Turmoil's virtual time and seeded RNG
//! 2. `SystemEnv` (kalandra-server): Uses real system clock and crypto-secure
//!    RNG
//!
//! # Invariants
//!
//! - Monotonicity: `env.now()` must never go backwards
//! - Determinism: Given the same seed, `random_bytes()` produces the same
//!   sequence
//! - Isolation: Implementations must not share global state

use std::time::{Duration, Instant};

/// Abstract environment providing time, randomness, and async primitives.
///
/// This trait is the foundation of the Sans-IO architecture. It allows
/// protocol logic to be completely deterministic and testable.
///
/// # Implementations
///
/// - Simulation (`kalandra-harness::SimEnv`): Virtual time that can be advanced
///   instantly, seeded RNG for reproducibility.
///
/// - Production (`kalandra-server::SystemEnv`): Real system clock,
///   crypto-secure RNG from OS entropy pool.
///
/// # Safety
///
/// Implementations MUST guarantee:
///
/// 1. Time monotonicity: `now()` never goes backwards
/// 2. RNG quality: `random_bytes()` uses cryptographically secure entropy in
///    production
/// 3. Minimal panics: Methods are infallible except in exceptional
///    circumstances (e.g., OS entropy exhaustion, incorrect simulation setup)
pub trait Environment: Clone + Send + Sync + 'static {
    /// Returns the current time.
    ///
    /// # Invariants
    ///
    /// - Monotonicity: This method MUST return values that never decrease
    ///   within a single execution context. Subsequent calls must return times
    ///   >= previous calls.
    ///
    /// # Implementation Notes
    ///
    /// - Simulation: Returns Turmoil's virtual time (via tokio::time which
    ///   turmoil intercepts)
    /// - Production: Returns `std::time::Instant::now()` (real clock)
    fn now(&self) -> Instant;

    /// Sleeps for the specified duration.
    ///
    /// # Behavior
    ///
    /// - Simulation: Advances Turmoil's virtual time instantly (no wall-clock
    ///   delay)
    /// - Production: Yields to Tokio scheduler for the specified duration
    ///
    /// # Implementation Notes
    ///
    /// This is the ONLY async method in the trait, and it should only be used
    /// by driver code (not protocol logic). Protocol logic should return
    /// "SetTimer" actions instead.
    fn sleep(&self, duration: Duration) -> impl std::future::Future<Output = ()> + Send;

    /// Fills the provided buffer with random bytes.
    ///
    /// # Invariants
    ///
    /// - Determinism during simulations: Given the same RNG seed, this produces
    ///   the same sequence of bytes
    /// - Unpredictability in production: Uses cryptographically secure RNG
    ///
    /// # Security
    ///
    /// Production implementations MUST use:
    /// - `getrandom::getrandom()` (OS entropy pool)
    /// - NOT `rand::thread_rng()` (not crypto-secure)
    ///
    /// Simulation implementations MUST use:
    /// - Turmoil's seeded RNG (`turmoil::lookup("rng")`)
    /// - The seed MUST be logged for reproducibility
    ///
    /// # Panics
    ///
    /// May panic if:
    /// - Running outside a Turmoil simulation (for `SimEnv`)
    /// - OS entropy pool is exhausted (extremely rare)
    fn random_bytes(&self, buffer: &mut [u8]);

    /// Generates a random `u64`.
    ///
    /// This is a convenience method for common use cases like generating
    /// session IDs or request IDs.
    fn random_u64(&self) -> u64 {
        let mut bytes = [0u8; 8];
        self.random_bytes(&mut bytes);
        u64::from_be_bytes(bytes)
    }

    /// Generates a random `u128`.
    ///
    /// Useful for UUIDs or room IDs.
    fn random_u128(&self) -> u128 {
        let mut bytes = [0u8; 16];
        self.random_bytes(&mut bytes);
        u128::from_be_bytes(bytes)
    }
}
