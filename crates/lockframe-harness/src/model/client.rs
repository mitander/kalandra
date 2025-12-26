//! Model client state machine.
//!
//! Simplified client that tracks room membership and received messages.
//! No cryptography - just the logical state transitions.

use std::collections::HashMap;

use super::operation::{ClientId, ModelRoomId, OperationError, OperationResult};

/// Message in the model (simplified).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelMessage {
    /// Sender's client ID.
    pub sender_id: ClientId,
    /// Message content.
    pub content: Vec<u8>,
    /// Log index in the room.
    pub log_index: u64,
}

/// Per-room state in the model client.
#[derive(Debug, Clone)]
struct ModelRoomState {
    /// Current epoch.
    epoch: u64,
    /// Messages received (ordered by log_index).
    messages: Vec<ModelMessage>,
    /// Next expected log index for sending.
    next_send_index: u64,
}

impl ModelRoomState {
    fn new() -> Self {
        Self { epoch: 0, messages: Vec::new(), next_send_index: 0 }
    }
}

/// Model client state.
///
/// Tracks room memberships and message state without real cryptography.
#[derive(Debug, Clone)]
pub struct ModelClient {
    /// Client identifier.
    id: ClientId,
    /// Active room memberships.
    rooms: HashMap<ModelRoomId, ModelRoomState>,
}

impl ModelClient {
    /// Create a new model client.
    pub fn new(id: ClientId) -> Self {
        Self { id, rooms: HashMap::new() }
    }

    /// Client identifier.
    pub fn id(&self) -> ClientId {
        self.id
    }

    /// Check if client is member of a room.
    pub fn is_member(&self, room_id: ModelRoomId) -> bool {
        self.rooms.contains_key(&room_id)
    }

    /// Current epoch for a room. `None` if not a member.
    pub fn epoch(&self, room_id: ModelRoomId) -> Option<u64> {
        self.rooms.get(&room_id).map(|r| r.epoch)
    }

    /// All rooms this client is a member of.
    pub fn rooms(&self) -> impl Iterator<Item = ModelRoomId> + '_ {
        self.rooms.keys().copied()
    }

    /// Messages received in a room (ordered by log_index).
    pub fn messages(&self, room_id: ModelRoomId) -> Option<&[ModelMessage]> {
        self.rooms.get(&room_id).map(|r| r.messages.as_slice())
    }

    /// Create a new room (client becomes sole member).
    pub fn create_room(&mut self, room_id: ModelRoomId) -> OperationResult {
        if self.rooms.contains_key(&room_id) {
            return OperationResult::Error(OperationError::RoomAlreadyExists);
        }

        self.rooms.insert(room_id, ModelRoomState::new());
        OperationResult::Ok
    }

    /// Leave a room.
    pub fn leave_room(&mut self, room_id: ModelRoomId) -> OperationResult {
        if self.rooms.remove(&room_id).is_none() {
            return OperationResult::Error(OperationError::NotMember);
        }

        OperationResult::Ok
    }

    /// Send a message to a room.
    ///
    /// Returns the log_index that will be assigned by the server.
    /// Note: The model client doesn't track sent content - that's the server's
    /// job.
    pub fn send_message(&mut self, room_id: ModelRoomId) -> Result<u64, OperationError> {
        let room = self.rooms.get_mut(&room_id).ok_or(OperationError::NotMember)?;

        let log_index = room.next_send_index;
        room.next_send_index += 1;

        Ok(log_index)
    }

    /// Receive a message (called by ModelWorld after server processes).
    pub fn receive_message(&mut self, room_id: ModelRoomId, message: ModelMessage) {
        if let Some(room) = self.rooms.get_mut(&room_id) {
            room.messages.push(message);
        }
    }

    /// Join a room (invited by another member).
    pub fn join_room(&mut self, room_id: ModelRoomId) -> OperationResult {
        if self.rooms.contains_key(&room_id) {
            return OperationResult::Error(OperationError::RoomAlreadyExists);
        }

        self.rooms.insert(room_id, ModelRoomState::new());
        OperationResult::Ok
    }

    /// Advance epoch for a room (after commit).
    pub fn advance_epoch(&mut self, room_id: ModelRoomId) {
        if let Some(room) = self.rooms.get_mut(&room_id) {
            room.epoch += 1;
        }
    }
}
