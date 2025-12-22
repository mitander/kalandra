//!  Client
//!
//! Action-based client state machine for the Lockframe protocol. Manages room
//! memberships, MLS group operations, and sender key encryption.
//!
//! # Architecture
//!
//! The client is a pure state machine that:
//! - Receives events from the caller (frames, ticks, application intents)
//! - Produces actions for the caller to execute (send frames, deliver messages)
//! - Uses the `Environment` trait for time and randomness (deterministic
//!   testing)
//!
//! # Components
//!
//! - [`Client`]: Top-level state machine managing multiple rooms
//! - [`SenderKeyStore`]: Per-room sender key ratchet management
//! - [`ClientEvent`]: Events fed into the client
//! - [`ClientAction`]: Actions produced by the client

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod client;
mod error;
mod event;
mod sender_key_store;

pub use client::{Client, ClientIdentity};
pub use error::ClientError;
pub use event::{ClientAction, ClientEvent, RoomStateSnapshot};
pub use lockframe_core::{
    env::Environment,
    mls::{MemberId, RoomId},
};
pub use sender_key_store::SenderKeyStore;
