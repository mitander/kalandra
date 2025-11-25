//! Sunder protocol core logic
//!
//! This crate contains the pure state machine logic for the Sunder protocol.
//! It is completely decoupled from I/O, enabling deterministic testing and
//! formal verification.
//!
//! # Architecture: "The Hollow Shell"
//!
//! Protocol logic is strictly separated from transport concerns:
//!
//! ```text
//! ┌────────────────────────────────────────┐
//! │  sunder-core                           │
//! │  - State machines                      │
//! │  - Protocol logic                      │
//! │  - Cryptographic operations            │
//! └────────────────────────────────────────┘
//!           ↓                 ↓
//! ┌──────────────────┐  ┌──────────────────┐
//! │ sunder-harness   │  │  sunder-server   │
//! │ (Turmoil)        │  │  (Quinn/Tokio)   │
//! │ - Virtual time   │  │  - Real network  │
//! │ - Seeded RNG     │  │  - System clock  │
//! │ - Fault inject   │  │  - Production    │
//! └──────────────────┘  └──────────────────┘
//! ```
//!
//! # Key Principles
//!
//! - **No I/O in Core**: Never call `tokio::spawn`,
//!   `std::time::Instant::now()`, or `rand::thread_rng()` directly
//! - **Environment Trait**: All side effects go through the `Environment` trait
//! - **Deterministic**: Given the same inputs and environment state, produce
//!   the same outputs
//!
//! # Modules
//!
//! - [`env`]: Environment abstraction (time, RNG)
//! - [`transport`]: Transport abstraction (streams)
//! - [`state`]: Protocol state machines (connection, MLS, etc.) [TODO]
//! - [`crypto`]: Cryptographic operations (sender keys, etc.) [TODO]

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod env;
pub mod transport;
