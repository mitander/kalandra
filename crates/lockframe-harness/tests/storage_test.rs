//! Storage integration tests with Oracle checks
//!
//! These tests verify the storage abstraction's critical invariants:
//! - Sequential log indices (no gaps)
//! - Monotonic growth
//! - Multi-room isolation
//! - MLS state persistence

use bytes::Bytes;
use lockframe_core::mls::MlsGroupState;
use lockframe_proto::{Frame, FrameHeader, Opcode};
use lockframe_server::{MemoryStorage, Storage, StorageError};

// Helper to create test frames
fn create_frame(room_id: u128, log_index: u64, sender_id: u64, epoch: u64) -> Frame {
    let mut header = FrameHeader::new(Opcode::AppMessage);
    header.set_room_id(room_id);
    header.set_log_index(log_index);
    header.set_sender_id(sender_id);
    header.set_epoch(epoch);

    Frame::new(header, Bytes::from(format!("payload-{}", log_index)))
}

// Oracle: Verify storage invariants
fn verify_storage_invariants(storage: &impl Storage, room_id: u128, expected_count: u64) {
    // Check latest log index
    let latest = storage.latest_log_index(room_id).expect("latest_log_index failed");

    if expected_count == 0 {
        assert_eq!(latest, None, "Expected no frames in room");
        return;
    }

    assert_eq!(latest, Some(expected_count - 1), "latest_log_index mismatch");

    // Load all frames
    let frames = storage.load_frames(room_id, 0, 10000).expect("load_frames failed");

    assert_eq!(frames.len(), expected_count as usize, "Frame count mismatch");

    // Verify sequential log_index (no gaps)
    for (i, frame) in frames.iter().enumerate() {
        assert_eq!(frame.header.log_index(), i as u64, "Gap in sequence at position {}", i);

        assert_eq!(frame.header.room_id(), room_id, "Frame in wrong room at position {}", i);
    }
}

#[test]
fn test_store_and_load_frames() {
    let storage = MemoryStorage::new();
    let room_id = 0x1234_5678_90ab_cdef_1234_5678_90ab_cdef;

    // Store 10 frames
    for i in 0..10 {
        let frame = create_frame(room_id, i, 100, 0);
        storage.store_frame(room_id, i, &frame).expect("store_frame failed");
    }

    // Oracle check
    verify_storage_invariants(&storage, room_id, 10);
}

#[test]
fn test_log_index_monotonic() {
    let storage = MemoryStorage::new();
    let room_id = 100;

    // Store frames 0, 1, 2
    for i in 0..3 {
        let frame = create_frame(room_id, i, 100, 0);
        storage.store_frame(room_id, i, &frame).expect("store failed");

        // Verify latest_log_index increases
        assert_eq!(storage.latest_log_index(room_id).expect("query failed"), Some(i));
    }

    verify_storage_invariants(&storage, room_id, 3);
}

#[test]
fn test_conflict_detection() {
    let storage = MemoryStorage::new();
    let room_id = 100;

    // Store frame 0
    let frame0 = create_frame(room_id, 0, 100, 0);
    storage.store_frame(room_id, 0, &frame0).expect("store failed");

    // Try to store frame 2 (gap!)
    let frame2 = create_frame(room_id, 2, 100, 0);
    let result = storage.store_frame(room_id, 2, &frame2);

    assert!(result.is_err(), "Expected error for gap in sequence");

    match result {
        Err(StorageError::Conflict { expected, got }) => {
            assert_eq!(expected, 1);
            assert_eq!(got, 2);
        },
        _ => panic!("Expected Conflict error"),
    }

    // Verify only frame 0 is stored
    verify_storage_invariants(&storage, room_id, 1);
}

#[test]
fn test_concurrent_rooms() {
    let storage = MemoryStorage::new();

    // Store frames in room 100
    for i in 0..5 {
        let frame = create_frame(100, i, 100, 0);
        storage.store_frame(100, i, &frame).expect("store failed");
    }

    // Store frames in room 200
    for i in 0..3 {
        let frame = create_frame(200, i, 200, 0);
        storage.store_frame(200, i, &frame).expect("store failed");
    }

    // Store frames in room 300
    for i in 0..7 {
        let frame = create_frame(300, i, 300, 0);
        storage.store_frame(300, i, &frame).expect("store failed");
    }

    // Verify each room independently
    verify_storage_invariants(&storage, 100, 5);
    verify_storage_invariants(&storage, 200, 3);
    verify_storage_invariants(&storage, 300, 7);

    // Verify room count
    assert_eq!(storage.room_count(), 3);
    assert_eq!(storage.total_frame_count(), 15);
}

#[test]
fn test_load_frames_pagination() {
    let storage = MemoryStorage::new();
    let room_id = 100;

    // Store 20 frames
    for i in 0..20 {
        let frame = create_frame(room_id, i, 100, 0);
        storage.store_frame(room_id, i, &frame).expect("store failed");
    }

    // Load first 10
    let batch1 = storage.load_frames(room_id, 0, 10).expect("load failed");
    assert_eq!(batch1.len(), 10);
    assert_eq!(batch1[0].header.log_index(), 0);
    assert_eq!(batch1[9].header.log_index(), 9);

    // Load next 10
    let batch2 = storage.load_frames(room_id, 10, 10).expect("load failed");
    assert_eq!(batch2.len(), 10);
    assert_eq!(batch2[0].header.log_index(), 10);
    assert_eq!(batch2[9].header.log_index(), 19);

    // Verify full sequence
    verify_storage_invariants(&storage, room_id, 20);
}

#[test]
fn test_mls_state_persistence() {
    let storage = MemoryStorage::new();
    let room_id = 0x1234_5678_90ab_cdef_1234_5678_90ab_cdef;

    // Initially no state
    assert_eq!(storage.load_mls_state(room_id).expect("load failed"), None);

    // Store initial state (epoch 0)
    let state0 = MlsGroupState::new(room_id, 0, [0u8; 32], vec![100, 200], vec![1, 2, 3, 4]);
    storage.store_mls_state(room_id, &state0).expect("store failed");

    // Load and verify
    let loaded0 =
        storage.load_mls_state(room_id).expect("load failed").expect("state should exist");
    assert_eq!(loaded0.epoch, 0);
    assert_eq!(loaded0.members, vec![100, 200]);

    // Update to epoch 1 (new member joins)
    let state1 =
        MlsGroupState::new(room_id, 1, [1u8; 32], vec![100, 200, 300], vec![5, 6, 7, 8, 9]);
    storage.store_mls_state(room_id, &state1).expect("store failed");

    // Load latest state
    let loaded1 =
        storage.load_mls_state(room_id).expect("load failed").expect("state should exist");
    assert_eq!(loaded1.epoch, 1);
    assert_eq!(loaded1.members, vec![100, 200, 300]);
}

#[test]
fn test_storage_clone_shares_state() {
    let storage1 = MemoryStorage::new();
    let room_id = 100;

    // Store frame via storage1
    let frame0 = create_frame(room_id, 0, 100, 0);
    storage1.store_frame(room_id, 0, &frame0).expect("store failed");

    // Clone storage
    let storage2 = storage1.clone();

    // Store frame via storage2
    let frame1 = create_frame(room_id, 1, 100, 0);
    storage2.store_frame(room_id, 1, &frame1).expect("store failed");

    // Both storages should see both frames (shared state via Arc)
    verify_storage_invariants(&storage1, room_id, 2);
    verify_storage_invariants(&storage2, room_id, 2);

    // Verify they're the same storage
    assert_eq!(storage1.total_frame_count(), 2);
    assert_eq!(storage2.total_frame_count(), 2);
}
