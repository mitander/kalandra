//! Room Manager
//!
//! Orchestrates MLS validation and frame sequencing for rooms.
//!
//! ## Responsibilities
//!
//! - Room Lifecycle: Create rooms with authorization metadata
//! - MLS Validation: Verify frames against group state before sequencing
//! - Frame Sequencing: Assign log indices for total ordering
//! - Action Generation: Return actions for driver to execute (action-based)
//!
//! ## Design
//!
//! - Explicit room creation: Prevents accidental rooms, enables future auth
//! - RoomMetadata: Extension point for permissions/roles (added later)
//! - Action-based: All methods return actions, no direct I/O

use std::collections::HashMap;

use kalandra_core::{
    env::Environment,
    mls::{MlsValidator, ValidationResult, error::MlsError, group::MlsGroup, state::MlsGroupState},
};
use kalandra_proto::{Frame, Opcode};

use crate::{
    sequencer::{Sequencer, SequencerAction, SequencerError},
    storage::{Storage, StorageError},
};

/// Metadata about a room (extension point for future authorization)
#[derive(Debug, Clone)]
pub struct RoomMetadata {
    /// User who created the room
    pub creator: u64, // UserId
    /// When the room was created
    pub created_at: std::time::Instant,
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
    room_metadata: HashMap<u128, RoomMetadata>,
}

/// Actions returned by RoomManager for driver to execute.
#[derive(Debug, Clone)]
pub enum RoomAction {
    /// Broadcast this frame to all room members
    Broadcast {
        /// Room ID to broadcast to
        room_id: u128,
        /// Frame to broadcast
        frame: Frame,
        /// Whether to exclude the original sender
        exclude_sender: bool,
        /// When the frame was processed by the server
        processed_at: std::time::Instant,
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
        processed_at: std::time::Instant,
    },

    /// Persist updated MLS state
    PersistMlsState {
        /// Room ID
        room_id: u128,
        /// Updated MLS state to persist
        state: MlsGroupState,
        /// When the state was updated
        processed_at: std::time::Instant,
    },

    /// Reject frame (send error to sender)
    Reject {
        /// Sender who should receive the rejection
        sender_id: u64,
        /// Reason for rejection
        reason: String,
        /// When the rejection occurred
        processed_at: std::time::Instant,
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
        processed_at: std::time::Instant,
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

    /// Epoch mismatch
    #[error("epoch mismatch: expected {expected}, got {actual}")]
    InvalidEpoch {
        /// Expected epoch number
        expected: u64,
        /// Actual epoch number received
        actual: u64,
    },

    /// Not a member of the group
    #[error("not a member: {0}")]
    NotMember(u64),
}

impl<E> RoomManager<E>
where
    E: Environment,
{
    /// Validate basic frame properties (epoch, membership) without signature
    /// verification This is done before sequencing to ensure the frame is
    /// worth processing
    fn validate_frame_basic(
        &self,
        frame: &Frame,
        group: &MlsGroup<E>,
        mls_state: Option<&MlsGroupState>,
    ) -> Result<(), RoomError> {
        let frame_epoch = frame.header.epoch();
        if frame_epoch != group.epoch() {
            return Err(RoomError::InvalidEpoch { expected: group.epoch(), actual: frame_epoch });
        }

        let sender_id = frame.header.sender_id();
        if let Some(state) = mls_state {
            if !state.is_member(sender_id) {
                return Err(RoomError::NotMember(sender_id));
            }
        }

        if let Some(state) = mls_state {
            // We do signature validation after sequencing
            // For now, just ensure the member has a key stored
            if state.member_key(sender_id).is_none() {
                return Err(RoomError::MlsValidation(
                    kalandra_core::mls::MlsError::MemberNotFound { member_id: sender_id },
                ));
            }
        } else {
            // No MLS state, expect epoch 0 frame
            if frame_epoch != 0 {
                return Err(RoomError::InvalidEpoch { expected: 0, actual: frame_epoch });
            }
        }

        Ok(())
    }

    /// Validate signatures on sequenced frames after they've been modified by
    /// the sequencer
    fn validate_sequenced_actions_signatures(
        &self,
        sequencer_actions: &[SequencerAction],
        mls_state: Option<&MlsGroupState>,
    ) -> Result<(), RoomError> {
        for action in sequencer_actions {
            let frame = match action {
                SequencerAction::AcceptFrame { frame, .. } => frame,
                SequencerAction::StoreFrame { frame, .. } => frame,
                SequencerAction::BroadcastToRoom { frame, .. } => frame,
                SequencerAction::RejectFrame { .. } => continue, // No signature validation needed
            };

            if let Some(opcode) = frame.header.opcode_enum() {
                match opcode {
                    Opcode::Commit | Opcode::Proposal | Opcode::Welcome => {
                        continue; // No signature validation needed
                    },
                    _ => {
                        // Validate signature for application messages
                        // (epoch and membership already checked in validate_frame_basic)
                        if let Some(state) = mls_state {
                            let validation_result = MlsValidator::validate_signature(frame, state)?;

                            if let ValidationResult::Reject { reason } = validation_result {
                                return Err(RoomError::MlsValidation(MlsError::ValidationFailed(
                                    reason,
                                )));
                            }
                        }
                    },
                }
            }
        }

        Ok(())
    }
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
    ) -> Result<RoomAction, RoomError> {
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
    ) -> Result<Vec<RoomAction>, RoomError> {
        let now = env.now();

        // 1. Room must exist (no lazy creation)
        let room_id = frame.header.room_id();
        let group = self.groups.get(&room_id).ok_or(RoomError::RoomNotFound(room_id))?;

        // 2. Basic frame validation (epoch, membership) - NOT signature yet
        let mls_state = storage.load_mls_state(room_id)?;
        self.validate_frame_basic(&frame, &group, mls_state.as_ref())?;

        // Check if this is a Commit before sequencing (we need the frame later)
        let is_commit = frame.header.opcode_enum() == Some(Opcode::Commit);
        let frame_for_mls = if is_commit { Some(frame.clone()) } else { None };

        // 3. Sequence the frame (assign log index) - this modifies context_id
        let sequencer_actions = self.sequencer.process_frame(frame, storage)?;

        // 4. NOW validate signatures on the sequenced frames
        self.validate_sequenced_actions_signatures(&sequencer_actions, mls_state.as_ref())?;

        // 4. Convert SequencerAction to RoomAction
        let mut room_actions: Vec<RoomAction> = sequencer_actions
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
                // MLS actions from peer commit are primarily logging (epoch advanced). These
                // are handled via tracing. The critical outcome is that
                // process_message() called merge_staged_commit() to advance our
                // epoch
                let commit_frame =
                    frame_for_mls.as_ref().expect("invariant: frame_for_mls is Some");
                let _mls_actions = group.process_message(commit_frame.clone())?;
            }

            // Export the updated MLS state for persistence
            let state = group.export_group_state()?;
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
