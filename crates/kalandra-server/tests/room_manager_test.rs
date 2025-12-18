//! Room Manager tests

use bytes::Bytes;
use kalandra_proto::{Frame, FrameHeader, Opcode};
use kalandra_server::{MemoryStorage, RoomAction, RoomError, RoomManager, Storage};

// Test environment using system RNG (std::time::Instant)
#[derive(Clone)]
struct TestEnv;

impl kalandra_core::env::Environment for TestEnv {
    type Instant = std::time::Instant;

    fn now(&self) -> Self::Instant {
        std::time::Instant::now()
    }

    fn sleep(&self, duration: std::time::Duration) -> impl std::future::Future<Output = ()> + Send {
        async move {
            tokio::time::sleep(duration).await;
        }
    }

    fn random_bytes(&self, buffer: &mut [u8]) {
        use rand::RngCore;
        rand::thread_rng().fill_bytes(buffer);
    }
}

#[test]
fn room_manager_new_has_no_rooms() {
    let manager = RoomManager::<TestEnv>::new();
    assert!(!manager.has_room(0x1234));
}

#[test]
fn create_room_succeeds_for_new_room() {
    let env = TestEnv;
    let mut manager = RoomManager::new();
    let room_id = 0x1234_5678_90ab_cdef_1234_5678_90ab_cdef;
    let creator = 42;

    let result = manager.create_room(room_id, creator, &env);
    assert!(result.is_ok());
    assert!(manager.has_room(room_id));
}

#[test]
fn create_room_rejects_duplicate() {
    let env = TestEnv;
    let mut manager = RoomManager::new();
    let room_id = 0x1234_5678_90ab_cdef_1234_5678_90ab_cdef;
    let creator = 42;

    // First creation succeeds
    manager.create_room(room_id, creator, &env).unwrap();

    // Second creation fails
    let result = manager.create_room(room_id, creator, &env);
    assert!(matches!(result, Err(RoomError::RoomAlreadyExists(_))));
}

#[test]
fn create_room_stores_metadata() {
    let env = TestEnv;
    let mut manager = RoomManager::new();
    let room_id = 0x1234_5678_90ab_cdef_1234_5678_90ab_cdef;
    let creator = 42;

    manager.create_room(room_id, creator, &env).unwrap();

    // Metadata should be stored (we'll verify this when we add getter methods)
    assert!(manager.has_room(room_id));
}

#[test]
fn create_multiple_rooms() {
    let env = TestEnv;
    let mut manager = RoomManager::new();

    let room1 = 0x1111_1111_1111_1111_1111_1111_1111_1111;
    let room2 = 0x2222_2222_2222_2222_2222_2222_2222_2222;
    let room3 = 0x3333_3333_3333_3333_3333_3333_3333_3333;

    manager.create_room(room1, 1, &env).unwrap();
    manager.create_room(room2, 2, &env).unwrap();
    manager.create_room(room3, 3, &env).unwrap();

    assert!(manager.has_room(room1));
    assert!(manager.has_room(room2));
    assert!(manager.has_room(room3));
}

#[test]
fn process_frame_rejects_unknown_room() {
    let env = TestEnv;
    let mut manager = RoomManager::<TestEnv>::new();
    let storage = MemoryStorage::new();

    // Create a frame for a room that doesn't exist
    let mut header = FrameHeader::new(Opcode::AppMessage);
    header.set_room_id(0x9999_9999_9999_9999_9999_9999_9999_9999);
    header.set_sender_id(42);
    header.set_epoch(0);
    let frame = Frame::new(header, Bytes::new());

    let result = manager.process_frame(frame, &env, &storage);
    assert!(matches!(result, Err(RoomError::RoomNotFound(_))));
}

#[test]
fn process_frame_succeeds_for_valid_frame() {
    let env = TestEnv;
    let mut manager = RoomManager::new();
    let storage = MemoryStorage::new();

    let room_id = 0x1234_5678_90ab_cdef_1234_5678_90ab_cdef;
    let creator = 42;

    // Create the room first
    manager.create_room(room_id, creator, &env).unwrap();

    // Create a valid frame
    let mut header = FrameHeader::new(Opcode::AppMessage);
    header.set_room_id(room_id);
    header.set_sender_id(creator);
    header.set_epoch(0);
    let frame = Frame::new(header, Bytes::new());

    let result = manager.process_frame(frame, &env, &storage);
    if let Err(ref e) = result {
        panic!("process_frame failed: {:?}", e);
    }
    assert!(result.is_ok());

    let actions = result.unwrap();
    // Should have actions (AcceptFrame becomes PersistFrame, StoreFrame becomes
    // PersistFrame, BroadcastToRoom becomes Broadcast) Sequencer returns 3
    // actions: AcceptFrame, StoreFrame, BroadcastToRoom
    assert!(!actions.is_empty());
    assert_eq!(actions.len(), 3);
}

#[test]
fn process_frame_returns_correct_action_types() {
    let env = TestEnv;
    let mut manager = RoomManager::new();
    let storage = MemoryStorage::new();

    let room_id = 0x1234_5678_90ab_cdef_1234_5678_90ab_cdef;
    let creator = 42;

    // Create the room first
    manager.create_room(room_id, creator, &env).unwrap();

    // Create a valid frame
    let mut header = FrameHeader::new(Opcode::AppMessage);
    header.set_room_id(room_id);
    header.set_sender_id(creator);
    header.set_epoch(0);
    let frame = Frame::new(header, Bytes::from("test message"));

    let result = manager.process_frame(frame, &env, &storage);
    assert!(result.is_ok());

    let actions = result.unwrap();

    // Verify we have the right action types
    // First two should be PersistFrame (from AcceptFrame and StoreFrame)
    assert!(matches!(actions[0], RoomAction::PersistFrame { .. }));
    assert!(matches!(actions[1], RoomAction::PersistFrame { .. }));

    // Last should be Broadcast (from BroadcastToRoom)
    assert!(matches!(actions[2], RoomAction::Broadcast { .. }));
}

#[test]
fn process_frame_rejects_wrong_epoch() {
    let env = TestEnv;
    let mut manager = RoomManager::new();
    let storage = MemoryStorage::new();

    let room_id = 0x1234_5678_90ab_cdef_1234_5678_90ab_cdef;
    let creator = 42;

    // Create room (epoch 0)
    manager.create_room(room_id, creator, &env).unwrap();

    // Store initial MLS state at epoch 0
    use kalandra_core::mls::MlsGroupState;
    let mls_state = MlsGroupState::new(room_id, 0, [0u8; 32], vec![creator], vec![]);
    storage.store_mls_state(room_id, &mls_state).unwrap();

    // Create frame with wrong epoch (epoch 5, but room is at epoch 0)
    let mut header = FrameHeader::new(Opcode::AppMessage);
    header.set_room_id(room_id);
    header.set_sender_id(creator);
    header.set_epoch(5); // Wrong epoch!
    let frame = Frame::new(header, Bytes::new());

    let result = manager.process_frame(frame, &env, &storage);
    assert!(matches!(result, Err(RoomError::MlsValidation(_))));
}

/// Test that RoomManager advances epoch after processing a Commit.
///
/// This test exposes a critical wiring bug: RoomManager validates frames
/// against MLS state but never updates the MLS state when processing commits.
/// After a commit advances the epoch, subsequent frames at the new epoch
/// should be accepted.
#[test]
fn process_commit_advances_epoch() {
    let env = TestEnv;
    let mut manager = RoomManager::new();
    let storage = MemoryStorage::new();

    let room_id = 0x1234_5678_90ab_cdef_1234_5678_90ab_cdef;
    let creator = 42;

    // Step 1: Create room (epoch 0)
    manager.create_room(room_id, creator, &env).unwrap();
    assert_eq!(manager.epoch(room_id), Some(0), "Room should start at epoch 0");

    // Step 2: Store initial MLS state for validation
    use kalandra_core::mls::MlsGroupState;
    let mls_state = MlsGroupState::new(room_id, 0, [0u8; 32], vec![creator], vec![]);
    storage.store_mls_state(room_id, &mls_state).unwrap();

    // Step 3: Generate a KeyPackage for a new member
    use kalandra_core::mls::MlsGroup;
    let new_member_id = 100u64;
    let (key_package_bytes, _hash) =
        MlsGroup::generate_key_package(env.clone(), new_member_id).expect("generate key package");

    // Step 4: Add member - creates a Commit and pending state
    use kalandra_core::mls::MlsAction;
    let add_actions =
        manager.add_members(room_id, &[key_package_bytes]).expect("add_members should succeed");

    // Epoch should still be 0 (commit not merged yet)
    assert_eq!(manager.epoch(room_id), Some(0), "Epoch should be 0 before commit processed");

    // Find the Commit frame from the actions
    let commit_frame = add_actions
        .iter()
        .find_map(|a| match a {
            MlsAction::SendCommit(frame) => Some(frame.clone()),
            _ => None,
        })
        .expect("add_members should produce a Commit frame");

    // Step 5: Process the commit through RoomManager (as if returned by sequencer)
    let mut header = commit_frame.header.clone();
    header.set_room_id(room_id);
    header.set_sender_id(creator);
    header.set_epoch(0); // Commit is sent at current epoch
    let commit_frame = Frame::new(header, commit_frame.payload);

    let result = manager.process_frame(commit_frame, &env, &storage);
    assert!(result.is_ok(), "process_frame should succeed");

    // CRITICAL ORACLE: Epoch should advance to 1 after processing the commit
    assert_eq!(
        manager.epoch(room_id),
        Some(1),
        "Epoch should advance to 1 after processing Commit"
    );

    // Step 6: Update MLS state in storage to reflect new epoch
    let mls_state = MlsGroupState::new(room_id, 1, [0u8; 32], vec![creator, new_member_id], vec![]);
    storage.store_mls_state(room_id, &mls_state).unwrap();

    // Step 7: Send a message at epoch 1 - should be accepted
    let mut header = FrameHeader::new(Opcode::AppMessage);
    header.set_room_id(room_id);
    header.set_sender_id(creator);
    header.set_epoch(1); // New epoch after commit
    let msg_frame = Frame::new(header, Bytes::from("message at epoch 1"));

    let result = manager.process_frame(msg_frame, &env, &storage);

    // ORACLE: Message at epoch 1 should be accepted
    assert!(
        result.is_ok(),
        "Message at epoch 1 should be accepted after commit. Error: {:?}",
        result.err()
    );
}

/// Test that handle_sync_request loads frames from storage and returns them.
#[test]
fn handle_sync_request_returns_stored_frames() {
    let env = TestEnv;
    let mut manager = RoomManager::new();
    let storage = MemoryStorage::new();

    let room_id = 0x1234_5678_90ab_cdef_1234_5678_90ab_cdef;
    let creator = 42;
    let requester = 100;

    // Create room
    manager.create_room(room_id, creator, &env).unwrap();

    // Store some frames directly in storage
    for i in 0..5 {
        let mut header = FrameHeader::new(Opcode::AppMessage);
        header.set_room_id(room_id);
        header.set_sender_id(creator);
        header.set_log_index(i);
        header.set_epoch(0);
        let frame = Frame::new(header, Bytes::from(format!("message {i}")));
        storage.store_frame(room_id, i, &frame).unwrap();
    }

    // Request sync from index 0
    let result = manager.handle_sync_request(room_id, requester, 0, 10, &env, &storage);
    assert!(result.is_ok());

    let action = result.unwrap();
    match action {
        RoomAction::SendSyncResponse {
            sender_id,
            room_id: rid,
            frames,
            has_more,
            server_epoch,
            ..
        } => {
            assert_eq!(sender_id, requester);
            assert_eq!(rid, room_id);
            assert_eq!(frames.len(), 5);
            assert!(!has_more);
            assert_eq!(server_epoch, 0);
        },
        _ => panic!("Expected SendSyncResponse action"),
    }
}

/// Test that handle_sync_request respects limit and sets has_more.
#[test]
fn handle_sync_request_paginates_with_limit() {
    let env = TestEnv;
    let mut manager = RoomManager::new();
    let storage = MemoryStorage::new();

    let room_id = 0x1234_5678_90ab_cdef_1234_5678_90ab_cdef;
    let creator = 42;

    // Create room
    manager.create_room(room_id, creator, &env).unwrap();

    // Store 10 frames
    for i in 0..10 {
        let mut header = FrameHeader::new(Opcode::AppMessage);
        header.set_room_id(room_id);
        header.set_sender_id(creator);
        header.set_log_index(i);
        header.set_epoch(0);
        let frame = Frame::new(header, Bytes::from(format!("message {i}")));
        storage.store_frame(room_id, i, &frame).unwrap();
    }

    // Request sync with limit of 3
    let result = manager.handle_sync_request(room_id, 100, 0, 3, &env, &storage);
    assert!(result.is_ok());

    let action = result.unwrap();
    match action {
        RoomAction::SendSyncResponse { frames, has_more, .. } => {
            assert_eq!(frames.len(), 3);
            assert!(has_more, "Should indicate more frames available");
        },
        _ => panic!("Expected SendSyncResponse action"),
    }

    // Request next batch starting from index 3
    let result = manager.handle_sync_request(room_id, 100, 3, 3, &env, &storage);
    assert!(result.is_ok());

    let action = result.unwrap();
    match action {
        RoomAction::SendSyncResponse { frames, has_more, .. } => {
            assert_eq!(frames.len(), 3);
            assert!(has_more, "Should still indicate more frames available");
        },
        _ => panic!("Expected SendSyncResponse action"),
    }
}

/// Test that handle_sync_request returns error for unknown room.
#[test]
fn handle_sync_request_unknown_room_fails() {
    let env = TestEnv;
    let manager = RoomManager::<TestEnv>::new();
    let storage = MemoryStorage::new();

    let result = manager.handle_sync_request(
        0x9999_9999_9999_9999_9999_9999_9999_9999,
        100,
        0,
        10,
        &env,
        &storage,
    );

    assert!(matches!(result, Err(RoomError::RoomNotFound(_))));
}
