//! Operations for model-based testing.
//!
//! Operations represent all possible actions in the system. They are generated
//! randomly by proptest and applied to both the model and real implementation.

use arbitrary::Arbitrary;

/// Client identifier (0-indexed).
pub type ClientId = u8;

/// Room identifier (uses u8 to keep test space manageable).
pub type ModelRoomId = u8;

/// Operations that can be applied to the system.
///
/// Each operation targets a specific client and may affect room state.
/// Operations are designed to be small and composable so proptest can
/// explore interesting combinations.
#[derive(Debug, Clone, Arbitrary)]
pub enum Operation {
    /// Client creates a new room.
    CreateRoom {
        /// Client performing the operation.
        client_id: ClientId,
        /// Room to create (mapped to u128 in real system).
        room_id: ModelRoomId,
    },

    /// Client sends a message to a room.
    SendMessage {
        /// Client sending the message.
        client_id: ClientId,
        /// Target room.
        room_id: ModelRoomId,
        /// Message content (kept small for efficiency).
        content: SmallMessage,
    },

    /// Client leaves a room.
    LeaveRoom {
        /// Client leaving.
        client_id: ClientId,
        /// Room to leave.
        room_id: ModelRoomId,
    },

    /// Advance simulation time.
    ///
    /// Triggers timeout processing in both model and real system.
    AdvanceTime {
        /// Milliseconds to advance.
        millis: u16,
    },

    /// Deliver pending messages.
    ///
    /// In real system, this processes network queue.
    /// In model, this is a no-op (instant delivery).
    DeliverPending,
}

/// Small message content for testing.
///
/// We use a compact representation to keep test cases small while still
/// exercising message handling. The content is deterministic from the seed.
#[derive(Debug, Clone, Arbitrary)]
pub struct SmallMessage {
    /// Message seed (expanded to content in real tests).
    pub seed: u8,
    /// Message length hint (0-3 maps to empty/small/medium/large).
    pub size_class: u8,
}

impl SmallMessage {
    /// Expand to actual message bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let len = match self.size_class % 4 {
            0 => 0,
            1 => 8,
            2 => 64,
            _ => 256,
        };

        // Deterministic content from seed
        (0..len).map(|i| self.seed.wrapping_add(i as u8)).collect()
    }
}

/// Result of applying an operation.
///
/// Used to compare model and real system behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationResult {
    /// Operation succeeded.
    Ok,

    /// Operation failed with expected error.
    Error(OperationError),
}

/// Expected errors that can occur during operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationError {
    /// Room already exists.
    RoomAlreadyExists,

    /// Room not found.
    RoomNotFound,

    /// Client is not a member of the room.
    NotMember,

    /// Invalid client ID.
    InvalidClient,
}

impl OperationResult {
    /// Check if operation succeeded.
    pub fn is_ok(&self) -> bool {
        matches!(self, OperationResult::Ok)
    }

    /// Check if operation failed.
    pub fn is_err(&self) -> bool {
        !self.is_ok()
    }
}
