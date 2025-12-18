//! Room Manager
//!
//! Orchestrates MLS validation and frame sequencing for rooms.
//!
//! ## Architecture
//!
//! ```text
//! Server
//!   ├─ Connections (session layer)
//!   ├─ RoomManager (group layer) ← THIS MODULE
//!   │   ├─ MlsGroups (per-room MLS state)
//!   │   └─ Sequencer (total ordering)
//!   └─ Storage (persistence)
//! ```
//!
//! ## Responsibilities
//!
//! 1. **Room Lifecycle**: Create rooms with authorization metadata
//! 2. **MLS Validation**: Verify frames against group state before sequencing
//! 3. **Frame Sequencing**: Assign log indices for total ordering
//! 4. **Action Generation**: Return actions for driver to execute (Sans-IO)
//!
//! ## Design Decisions
//!
//! - **Explicit room creation**: Prevents accidental rooms, enables future auth
//! - **RoomMetadata**: Extension point for permissions/roles (added later)
//! - **Sans-IO**: All methods return actions, no direct I/O
//! - **Generic over Instant**: Works with any time abstraction

use std::collections::HashMap;

use kalandra_core::{
    env::Environment,
    mls::{error::MlsError, group::MlsGroup, state::MlsGroupState},
};
use kalandra_proto::{Frame, Opcode};

use crate::{
    sequencer::{Sequencer, SequencerAction, SequencerError},
    storage::{Storage, StorageError},
};

/// Metadata about a room (extension point for future authorization)
#[derive(Debug, Clone)]
pub struct RoomMetadata<I> {
    /// User who created the room
    pub creator: u64, // UserId
    /// When the room was created
    pub created_at: I,
    // Future: admins, members, permissions
}

/// Orchestrates MLS validation + frame sequencing per room
pub struct RoomManager<E>
where
    E: Environment,
{
    /// Per-room MLS group state
    groups: HashMap<u128, MlsGroup<E>>,
    /// Frame sequencer (assigns log indices)
    sequencer: Sequencer,
    /// Room metadata (for future authorization)
    room_metadata: HashMap<u128, RoomMetadata<E::Instant>>,
}

/// Actions returned by RoomManager for driver to execute.
///
/// Generic over `I` (Instant type) to support both real and virtual time.
#[derive(Debug, Clone)]
pub enum RoomAction<I> {
    /// Broadcast this frame to all room members
    Broadcast {
        /// Room ID to broadcast to
        room_id: u128,
        /// Frame to broadcast
        frame: Frame,
        /// Whether to exclude the original sender
        exclude_sender: bool,
        /// When the frame was processed by the server
        processed_at: I,
    },

    /// Persist frame to storage
    PersistFrame {
        /// Room ID
        room_id: u128,
        /// Log index for this frame
        log_index: u64,
        /// Frame to persist
        frame: Frame,
        /// When the frame was processed by the server
        processed_at: I,
    },

    /// Persist updated MLS state
    PersistMlsState {
        /// Room ID
        room_id: u128,
        /// Updated MLS state to persist
        state: MlsGroupState,
        /// When the state was updated
        processed_at: I,
    },

    /// Reject frame (send error to sender)
    Reject {
        /// Sender who should receive the rejection
        sender_id: u64,
        /// Reason for rejection
        reason: String,
        /// When the rejection occurred
        processed_at: I,
    },

    /// Send sync response to client
    SendSyncResponse {
        /// Sender to reply to
        sender_id: u64,
        /// Room ID the sync is for
        room_id: u128,
        /// Raw frame bytes to send (each frame serialized)
        frames: Vec<Vec<u8>>,
        /// Whether more frames are available
        has_more: bool,
        /// Current epoch for this room
        server_epoch: u64,
        /// When the response was prepared
        processed_at: I,
    },
}

/// Errors from RoomManager operations
#[derive(Debug, thiserror::Error)]
pub enum RoomError {
    /// MLS validation failed
    #[error("MLS validation failed: {0}")]
    MlsValidation(#[from] MlsError),

    /// Sequencer error occurred
    #[error("Sequencer error: {0}")]
    Sequencing(#[from] SequencerError),

    /// Storage error occurred
    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),

    /// Room does not exist
    #[error("Room not found: {0:032x}")]
    RoomNotFound(u128),

    /// Room already exists
    #[error("Room already exists: {0:032x}")]
    RoomAlreadyExists(u128),
}

impl<E> RoomManager<E>
where
    E: Environment,
{
    /// Create a new RoomManager
    pub fn new() -> Self {
        Self { groups: HashMap::new(), sequencer: Sequencer::new(), room_metadata: HashMap::new() }
    }

    /// Check if a room exists
    pub fn has_room(&self, room_id: u128) -> bool {
        self.room_metadata.contains_key(&room_id)
    }

    /// Get the current epoch for a room.
    ///
    /// Returns `None` if the room doesn't exist.
    pub fn epoch(&self, room_id: u128) -> Option<u64> {
        self.groups.get(&room_id).map(|g| g.epoch())
    }

    /// Creates a room with the specified ID and records the creator for
    /// future authorization checks. Prevents duplicate room creation.
    ///
    /// # Errors
    ///
    /// Returns `RoomError::RoomAlreadyExists` if the room ID already exists.
    pub fn create_room(&mut self, room_id: u128, creator: u64, env: &E) -> Result<(), RoomError> {
        if self.has_room(room_id) {
            return Err(RoomError::RoomAlreadyExists(room_id));
        }

        // Create MLS group with Environment
        // For server-side room creation, we use room_id as member_id (server is initial
        // member)
        let (group, _actions) =
            MlsGroup::new(env.clone(), room_id, creator).map_err(RoomError::MlsValidation)?;
        self.groups.insert(room_id, group);

        // Store metadata (placeholder for future auth)
        let metadata = RoomMetadata { creator, created_at: env.now() };
        self.room_metadata.insert(room_id, metadata);

        Ok(())
    }

    /// Add members to a room by their KeyPackages.
    ///
    /// Creates MLS commits and welcomes for adding new members.
    /// The returned actions should be executed by the driver.
    ///
    /// # Errors
    ///
    /// Returns `RoomError::RoomNotFound` if the room doesn't exist.
    /// Returns `RoomError::MlsValidation` if MLS operations fail.
    pub fn add_members(
        &mut self,
        room_id: u128,
        key_packages: &[Vec<u8>],
    ) -> Result<Vec<kalandra_core::mls::MlsAction>, RoomError> {
        let group = self.groups.get_mut(&room_id).ok_or(RoomError::RoomNotFound(room_id))?;
        let actions = group.add_members_from_bytes(key_packages)?;
        Ok(actions)
    }

    /// Remove members from a room by their member IDs.
    ///
    /// Creates an MLS commit to remove the specified members.
    /// The returned actions should be executed by the driver.
    ///
    /// # Errors
    ///
    /// Returns `RoomError::RoomNotFound` if the room doesn't exist.
    /// Returns `RoomError::MlsValidation` if any member ID is not found
    /// or if the caller tries to remove themselves (use `leave_room` instead).
    pub fn remove_members(
        &mut self,
        room_id: u128,
        member_ids: &[u64],
    ) -> Result<Vec<kalandra_core::mls::MlsAction>, RoomError> {
        let group = self.groups.get_mut(&room_id).ok_or(RoomError::RoomNotFound(room_id))?;
        let actions = group.remove_members(member_ids)?;
        Ok(actions)
    }

    /// Leave a room voluntarily.
    ///
    /// Creates an MLS Remove proposal for self-removal. In MLS, members
    /// cannot unilaterally remove themselves - another member must commit
    /// the removal.
    ///
    /// # Errors
    ///
    /// Returns `RoomError::RoomNotFound` if the room doesn't exist.
    /// Returns `RoomError::MlsValidation` if proposal creation fails.
    pub fn leave_room(
        &mut self,
        room_id: u128,
    ) -> Result<Vec<kalandra_core::mls::MlsAction>, RoomError> {
        let group = self.groups.get_mut(&room_id).ok_or(RoomError::RoomNotFound(room_id))?;
        let actions = group.leave_group()?;
        Ok(actions)
    }

    /// Handle a sync request from a client.
    ///
    /// Loads frames from storage starting at `from_log_index` and returns
    /// a `SendSyncResponse` action for the driver to send back to the client.
    ///
    /// # Protocol Flow
    ///
    /// 1. Client detects epoch mismatch or commit timeout
    /// 2. Client sends SyncRequest with `from_log_index`
    /// 3. Server calls this method to load frames from storage
    /// 4. Server sends SyncResponse with frames batch
    /// 5. Client processes frames in order to catch up
    /// 6. If `has_more` is true, client sends another SyncRequest
    ///
    /// # Errors
    ///
    /// Returns `RoomError::RoomNotFound` if the room doesn't exist.
    /// Returns `RoomError::Storage` if frame loading fails.
    pub fn handle_sync_request(
        &self,
        room_id: u128,
        sender_id: u64,
        from_log_index: u64,
        limit: usize,
        env: &E,
        storage: &impl Storage,
    ) -> Result<RoomAction<E::Instant>, RoomError> {
        let now = env.now();

        let group = self.groups.get(&room_id).ok_or(RoomError::RoomNotFound(room_id))?;
        let server_epoch = group.epoch();

        let frames = storage.load_frames(room_id, from_log_index, limit)?;

        let frame_bytes: Vec<Vec<u8>> = frames
            .iter()
            .map(|f| {
                let mut buf = Vec::new();
                // Frame encoding should not fail for valid frames
                f.encode(&mut buf).expect("invariant: stored frames are valid");
                buf
            })
            .collect();

        let latest_index = storage.latest_log_index(room_id)?;
        let last_loaded_index = if frames.is_empty() {
            from_log_index.saturating_sub(1)
        } else {
            from_log_index + frames.len() as u64 - 1
        };
        let has_more = latest_index.is_some_and(|latest| last_loaded_index < latest);

        Ok(RoomAction::SendSyncResponse {
            sender_id,
            room_id,
            frames: frame_bytes,
            has_more,
            server_epoch,
            processed_at: now,
        })
    }

    /// Process a frame through MLS validation and sequencing
    ///
    /// This method orchestrates the full frame processing pipeline:
    /// 1. Verify room exists (no lazy creation)
    /// 2. Validate frame against MLS state
    /// 3. Sequence the frame (assign log index)
    /// 4. Convert SequencerAction to RoomAction
    /// 5. Return actions for driver to execute
    ///
    /// # Errors
    ///
    /// Returns `RoomError::RoomNotFound` if room doesn't exist.
    /// Returns `RoomError::MlsValidation` if frame fails validation.
    /// Returns `RoomError::Sequencing` if sequencer encounters an error.
    pub fn process_frame(
        &mut self,
        frame: Frame,
        env: &E,
        storage: &impl Storage,
    ) -> Result<Vec<RoomAction<E::Instant>>, RoomError> {
        let now = env.now();

        // 1. Room must exist (no lazy creation)
        let room_id = frame.header.room_id();
        let group = self.groups.get(&room_id).ok_or(RoomError::RoomNotFound(room_id))?;

        // 2. Validate frame against MLS state
        let mls_state = storage.load_mls_state(room_id)?;
        group.validate_frame(&frame, mls_state.as_ref())?;

        // Check if this is a Commit before sequencing (we need the frame later)
        let is_commit = frame.header.opcode_enum() == Some(Opcode::Commit);
        let frame_for_mls = if is_commit { Some(frame.clone()) } else { None };

        // 3. Sequence the frame (assign log index)
        let sequencer_actions = self.sequencer.process_frame(frame, storage)?;

        // 4. Convert SequencerAction to RoomAction
        let mut room_actions: Vec<RoomAction<E::Instant>> = sequencer_actions
            .into_iter()
            .map(|action| match action {
                SequencerAction::AcceptFrame { room_id, log_index, frame } => {
                    RoomAction::PersistFrame { room_id, log_index, frame, processed_at: now }
                },
                SequencerAction::StoreFrame { room_id, log_index, frame } => {
                    RoomAction::PersistFrame { room_id, log_index, frame, processed_at: now }
                },
                SequencerAction::BroadcastToRoom { room_id, frame } => RoomAction::Broadcast {
                    room_id,
                    frame,
                    exclude_sender: false,
                    processed_at: now,
                },
                SequencerAction::RejectFrame { room_id: _, reason, original_frame } => {
                    RoomAction::Reject {
                        sender_id: original_frame.header.sender_id(),
                        reason,
                        processed_at: now,
                    }
                },
            })
            .collect();

        // 5. Update MLS state if this was a Commit
        if frame_for_mls.is_some() {
            let group = self.groups.get_mut(&room_id).ok_or(RoomError::RoomNotFound(room_id))?;

            if group.has_mls_pending_commit() {
                // We created this commit - merge our pending state
                group.merge_pending_commit()?;
            } else {
                // TODO: Process commits from other senders. Right now, the
                // server is always the commiter.
            }

            // Export the updated MLS state for persistence
            let state = group.export_group_state();
            room_actions.push(RoomAction::PersistMlsState { room_id, state, processed_at: now });
        }

        Ok(room_actions)
    }
}

impl<E> Default for RoomManager<E>
where
    E: Environment,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<E> std::fmt::Debug for RoomManager<E>
where
    E: Environment,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RoomManager")
            .field("room_count", &self.room_metadata.len())
            .field("sequencer", &self.sequencer)
            .finish()
    }
}
