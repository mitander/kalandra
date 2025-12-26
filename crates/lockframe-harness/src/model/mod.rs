//! Reference model for model-based testing.
//!
//! The model is a simplified implementation that captures the SPECIFICATION
//! of the Lockframe protocol without the complexity of real cryptography.
//! It serves as the oracle against which the real implementation is verified.
//!
//! # Design Principles
//!
//! - Simplicity: The model should be obviously correct
//! - Specification not implementation: Captures WHAT, not HOW
//! - Deterministic: Same inputs produce same outputs

mod client;
pub mod operation;
mod server;
mod world;

pub use client::{ModelClient, ModelMessage};
pub use operation::{
    ClientId, ModelRoomId, Operation, OperationError, OperationResult, SmallMessage,
};
pub use server::ModelServer;
pub use world::{ModelWorld, ObservableState};
