//! MLS (Messaging Layer Security) implementation.
//!
//! This module implements the MLS protocol (RFC 9420) for group messaging with
//! strong security guarantees including forward secrecy and post-compromise
//! security.
//!
//! # Components
//!
//! - [`group`]: Client-side MLS group state machine
//! - [`state`]: MLS group state for storage and validation
//! - [`provider`]: OpenMLS provider integration
//! - [`validator`]: Frame validation for server sequencing
//! - [`error`]: MLS-specific error types
//! - [`constants`]: Protocol constants and limits
//!
//! # Design
//!
//! 1. Sans-IO: All MLS logic returns actions, no direct I/O
//! 2. Epoch-based ordering: Server enforces total order via epochs
//! 3. Server authority: Server can moderate via External Commits
//! 4. Action pattern: Methods return `Result<Vec<MlsAction>, MlsError>`

pub mod constants;
pub mod error;
pub mod group;
pub mod provider;
pub mod state;
pub mod validator;

pub use constants::MAX_EPOCH;
pub use error::MlsError;
pub use group::{MemberId, MlsAction, MlsGroup, PendingJoinState, RoomId};
pub use provider::MlsProvider;
pub use state::MlsGroupState;
pub use validator::{MlsValidator, ValidationResult};
