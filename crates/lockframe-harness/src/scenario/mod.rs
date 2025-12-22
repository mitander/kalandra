//! Scenario testing framework for deterministic simulation tests.
//!
//! This module provides a declarative API for writing scenario-based tests
//! that follow the Oracle Pattern. Scenarios automatically handle network I/O,
//! action execution, and enforce oracle verification.

mod actor;
mod builder;
pub mod oracle;
mod world;

pub use actor::{ClientActor, ServerActor};
pub use builder::{RunnableScenario, Scenario};
pub use oracle::OracleFn;
pub use world::World;
