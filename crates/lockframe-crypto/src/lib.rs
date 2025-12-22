//! Lockframe Cryptographic Primitives
//!
//! This crate provides the cryptographic building blocks for the Lockframe
//! protocol.
//!
//! # Design
//!
//! All functions in this crate are pure - they have no side effects and
//! produce deterministic outputs given the same inputs. Random bytes required
//! for encryption must be provided by the caller, enabling:
//!
//! - Deterministic testing with seeded RNG
//! - Sans-IO architecture compatibility
//! - No coupling to application-level abstractions
//!
//! # Security Properties
//!
//! - Forward Secrecy: Old chain keys are deleted after deriving the next one
//! - Post-Compromise Security: New MLS epoch = new sender keys
//! - Sender Authentication: Each sender has unique keys derived from their
//!   index

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod sender_keys;

pub use sender_keys::{
    EncryptedMessage, MessageKey, NONCE_RANDOM_SIZE, SenderKeyError, SymmetricRatchet,
    decrypt_message, derive_sender_key_seed, encrypt_message,
};
