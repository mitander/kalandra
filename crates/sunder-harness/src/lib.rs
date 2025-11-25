//! Deterministic simulation harness for Sunder protocol testing.
//!
//! This crate provides Turmoil-based implementations of the `Environment`
//! and `Transport` traits, enabling deterministic, reproducible testing
//! of the Sunder protocol under various network conditions.
//!
//! # Why Deterministic Simulation?
//!
//! Traditional integration tests with real networks suffer from:
//!
//! - **Non-reproducibility**: Race conditions that only appear occasionally
//! - **Limited coverage**: Can't easily test rare network faults
//! - **Slow execution**: Real timeouts waste wall-clock time
//!
//! Deterministic simulation solves these problems:
//!
//! - **Perfect reproducibility**: Given the same seed, get the same execution
//! - **Fault injection**: Easily test packet loss, partitions, reordering
//! - **Fast execution**: Virtual time advances instantly
//!
//! # Example
//!
//! ```rust,ignore
//! use sunder_harness::{SimEnv, SimTransport};
//! use turmoil::Builder;
//!
//! #[test]
//! fn test_handshake() {
//!     let mut sim = Builder::new().build();
//!
//!     sim.host("server", || async {
//!         let env = SimEnv;
//!         let transport = SimTransport::bind("0.0.0.0:443").await?;
//!         // Server logic...
//!         Ok(())
//!     });
//!
//!     sim.client("client", || async {
//!         let env = SimEnv;
//!         // Client logic...
//!         Ok(())
//!     });
//!
//!     sim.run().unwrap();
//! }
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod sim_env;
mod sim_transport;

pub use sim_env::SimEnv;
pub use sim_transport::SimTransport;
