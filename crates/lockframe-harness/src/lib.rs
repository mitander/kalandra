//! Deterministic simulation harness for Lockframe protocol testing.
//!
//! Turmoil-based implementations of the Environment and Transport traits for
//! deterministic, reproducible testing under various network conditions.
//!
//! # Model-Based Testing
//!
//! The `model` module provides a reference implementation for model-based
//! testing. Operations are applied to both the model and real implementation,
//! and their observable states are compared.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod model;
pub mod scenario;
pub mod sim_env;
pub mod sim_server;
pub mod sim_transport;

pub use model::{
    ClientId, ModelClient, ModelMessage, ModelRoomId, ModelServer, ModelWorld, ObservableState,
    Operation, OperationError, OperationResult, SmallMessage,
};
pub use sim_env::SimEnv;
pub use sim_server::{SharedSimServer, SimServer, create_shared_server};
pub use sim_transport::SimTransport;
