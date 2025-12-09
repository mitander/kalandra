//! Client error types.

use kalandra_core::mls::RoomId;
use kalandra_crypto::SenderKeyError;
use thiserror::Error;

/// Errors from client operations.
#[derive(Debug, Error)]
pub enum ClientError {
    /// Room not found in client state.
    #[error("room not found: {room_id:x}")]
    RoomNotFound {
        /// The room ID that was not found.
        room_id: RoomId,
    },

    /// Frame epoch doesn't match room's current epoch.
    #[error("epoch mismatch: expected {expected}, got {actual}")]
    EpochMismatch {
        /// Expected epoch (room's current epoch).
        expected: u64,
        /// Actual epoch in the frame.
        actual: u64,
    },

    /// Room already exists.
    #[error("room already exists: {room_id:x}")]
    RoomAlreadyExists {
        /// The room ID that already exists.
        room_id: RoomId,
    },

    /// MLS operation failed.
    #[error("MLS error: {reason}")]
    Mls {
        /// Description of the MLS failure.
        reason: String,
    },

    /// Sender key operation failed.
    #[error("sender key error: {0}")]
    SenderKey(#[from] SenderKeyError),

    /// Frame parsing or validation failed.
    #[error("invalid frame: {reason}")]
    InvalidFrame {
        /// Description of the frame error.
        reason: String,
    },

    /// Client is in an invalid state for the operation.
    #[error("invalid state: {reason}")]
    InvalidState {
        /// Description of the state error.
        reason: String,
    },

    /// Sync required to process frame.
    #[error("sync required: room {room_id:x} needs epoch {target_epoch}")]
    SyncRequired {
        /// Room that needs syncing.
        room_id: RoomId,
        /// Target epoch to sync to.
        target_epoch: u64,
    },
}

impl ClientError {
    /// Returns true if this error is fatal (unrecoverable).
    ///
    /// Fatal errors indicate protocol violations or bugs.
    /// Transient errors can be recovered via sync or retry.
    pub fn is_fatal(&self) -> bool {
        match self {
            // Fatal: protocol violations, crypto failures
            Self::InvalidFrame { .. } | Self::InvalidState { .. } | Self::Mls { .. } => true,

            // Fatal sender key errors
            Self::SenderKey(e) => e.is_fatal(),

            // Transient: can be recovered
            Self::RoomNotFound { .. }
            | Self::RoomAlreadyExists { .. }
            | Self::EpochMismatch { .. }
            | Self::SyncRequired { .. } => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_not_found_is_transient() {
        let err = ClientError::RoomNotFound { room_id: 123 };
        assert!(!err.is_fatal());
    }

    #[test]
    fn invalid_frame_is_fatal() {
        let err = ClientError::InvalidFrame { reason: "bad magic".to_string() };
        assert!(err.is_fatal());
    }

    #[test]
    fn sync_required_is_transient() {
        let err = ClientError::SyncRequired { room_id: 123, target_epoch: 5 };
        assert!(!err.is_fatal());
    }

    #[test]
    fn error_display() {
        let err = ClientError::EpochMismatch { expected: 5, actual: 3 };
        assert_eq!(err.to_string(), "epoch mismatch: expected 5, got 3");
    }
}
