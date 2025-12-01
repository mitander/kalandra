//! Integration tests for the Sequencer with Oracle checks
//!
//! These tests verify total ordering invariants under various scenarios:
//! - Single client sequencing
//! - Concurrent clients
//! - Epoch boundaries
//! - Crash recovery
//!
//! # Oracle Pattern
//!
//! Each test ends with an Oracle function that verifies global consistency:
//! - No gaps in log indices
//! - Monotonic ordering
//! - Epoch transitions are valid

use bytes::Bytes;
use kalandra_core::{
    mls::{MlsGroupState, MlsValidator},
    sequencer::{Sequencer, SequencerAction},
    storage::{MemoryStorage, Storage},
};
use kalandra_proto::{Frame, FrameHeader, Opcode};

/// Helper: Create a test frame
fn create_test_frame(room_id: u128, sender_id: u64, epoch: u64, payload: &str) -> Frame {
    let mut header = FrameHeader::new(Opcode::AppMessage);
    header.set_room_id(room_id);
    header.set_sender_id(sender_id);
    header.set_epoch(epoch);

    Frame::new(header, Bytes::from(payload.to_string()))
}

/// Helper: Create MLS state for testing
fn create_test_state(room_id: u128, epoch: u64, members: Vec<u64>) -> MlsGroupState {
    MlsGroupState::new(room_id, epoch, [0u8; 32], members, vec![])
}

/// Oracle: Verify sequential log indices with no gaps
fn verify_sequential_indices(storage: &MemoryStorage, room_id: u128, expected_count: usize) {
    let frames = storage.load_frames(room_id, 0, expected_count + 10).expect("load_frames failed");

    assert_eq!(
        frames.len(),
        expected_count,
        "expected {} frames, got {}",
        expected_count,
        frames.len()
    );

    for (i, frame) in frames.iter().enumerate() {
        assert_eq!(
            frame.header.log_index(),
            i as u64,
            "gap detected: expected log_index={}, got={}",
            i,
            frame.header.log_index()
        );
    }
}

/// Oracle: Verify all frames have correct epoch
fn verify_epoch_consistency(storage: &MemoryStorage, room_id: u128, expected_epoch: u64) {
    let frames = storage.load_frames(room_id, 0, 1000).expect("load_frames failed");

    for frame in frames {
        assert_eq!(
            frame.header.epoch(),
            expected_epoch,
            "frame has wrong epoch: expected {}, got {}",
            expected_epoch,
            frame.header.epoch()
        );
    }
}

#[test]
fn test_single_client_sequencing() {
    let mut sequencer = Sequencer::new();
    let storage = MemoryStorage::new();
    let validator = MlsValidator;

    let room_id = 100;
    let sender_id = 200;

    // Initialize room with MLS state
    let state = create_test_state(room_id, 0, vec![sender_id]);
    storage.store_mls_state(room_id, &state).expect("store_mls_state failed");

    // Send 5 frames from single client
    for i in 0..5 {
        let frame = create_test_frame(room_id, sender_id, 0, &format!("msg-{}", i));
        let actions =
            sequencer.process_frame(frame, &storage, &validator).expect("process_frame failed");

        // Execute StoreFrame action
        for action in actions {
            if let SequencerAction::StoreFrame { room_id, log_index, frame } = action {
                storage.store_frame(room_id, log_index, &frame).expect("store_frame failed");
            }
        }
    }

    // Oracle: Verify sequential indices
    verify_sequential_indices(&storage, room_id, 5);

    // Oracle: Verify all frames have epoch 0
    verify_epoch_consistency(&storage, room_id, 0);
}

#[test]
fn test_concurrent_clients() {
    let mut sequencer = Sequencer::new();
    let storage = MemoryStorage::new();
    let validator = MlsValidator;

    let room_id = 100;
    let client_a = 200;
    let client_b = 300;

    // Initialize room with 2 members
    let state = create_test_state(room_id, 0, vec![client_a, client_b]);
    storage.store_mls_state(room_id, &state).expect("store_mls_state failed");

    // Interleave frames from two clients
    let frames = vec![
        (client_a, "a1"),
        (client_b, "b1"),
        (client_a, "a2"),
        (client_b, "b2"),
        (client_a, "a3"),
        (client_b, "b3"),
    ];

    for (sender, payload) in frames {
        let frame = create_test_frame(room_id, sender, 0, payload);
        let actions =
            sequencer.process_frame(frame, &storage, &validator).expect("process_frame failed");

        // Execute StoreFrame action
        for action in actions {
            if let SequencerAction::StoreFrame { room_id, log_index, frame } = action {
                storage.store_frame(room_id, log_index, &frame).expect("store_frame failed");
            }
        }
    }

    // Oracle: Verify no gaps despite concurrent clients
    verify_sequential_indices(&storage, room_id, 6);

    // Oracle: Verify total ordering (payloads are interleaved correctly)
    let stored_frames = storage.load_frames(room_id, 0, 10).expect("load_frames failed");
    let payloads: Vec<String> =
        stored_frames.iter().map(|f| String::from_utf8_lossy(&f.payload).to_string()).collect();

    assert_eq!(payloads, vec!["a1", "b1", "a2", "b2", "a3", "b3"]);
}

#[test]
fn test_epoch_boundary() {
    let mut sequencer = Sequencer::new();
    let storage = MemoryStorage::new();
    let validator = MlsValidator;

    let room_id = 100;
    let sender_id = 200;

    // Start at epoch 0
    let state = create_test_state(room_id, 0, vec![sender_id]);
    storage.store_mls_state(room_id, &state).expect("store_mls_state failed");

    // Send 3 frames at epoch 0
    for i in 0..3 {
        let frame = create_test_frame(room_id, sender_id, 0, &format!("epoch0-{}", i));
        let actions =
            sequencer.process_frame(frame, &storage, &validator).expect("process_frame failed");

        for action in actions {
            if let SequencerAction::StoreFrame { room_id, log_index, frame } = action {
                storage.store_frame(room_id, log_index, &frame).expect("store_frame failed");
            }
        }
    }

    // Simulate epoch transition (new commit advances epoch)
    let new_state = create_test_state(room_id, 1, vec![sender_id]);
    storage.store_mls_state(room_id, &new_state).expect("store_mls_state failed");

    // Update sequencer's cached epoch (simulate restart or cache invalidation)
    // In production, this would happen via commit processing
    let mut new_sequencer = Sequencer::new(); // Fresh sequencer loads epoch 1 from storage

    // Send frame with old epoch (should be rejected)
    let old_epoch_frame = create_test_frame(room_id, sender_id, 0, "stale");
    let actions = new_sequencer
        .process_frame(old_epoch_frame, &storage, &validator)
        .expect("process_frame failed");

    // Oracle: Verify rejection
    assert_eq!(actions.len(), 1);
    match &actions[0] {
        SequencerAction::RejectFrame { reason, .. } => {
            assert!(reason.contains("epoch mismatch"), "got: {}", reason);
        },
        _ => panic!("expected RejectFrame, got: {:?}", actions[0]),
    }

    // Send frame with correct epoch (should be accepted)
    let new_epoch_frame = create_test_frame(room_id, sender_id, 1, "epoch1-0");
    let actions = new_sequencer
        .process_frame(new_epoch_frame, &storage, &validator)
        .expect("process_frame failed");

    for action in actions {
        if let SequencerAction::StoreFrame { room_id, log_index, frame } = action {
            storage.store_frame(room_id, log_index, &frame).expect("store_frame failed");
        }
    }

    // Oracle: Verify sequential indices continue (3 from epoch 0, 1 from epoch 1)
    verify_sequential_indices(&storage, room_id, 4);
}

#[test]
fn test_sequencer_restart() {
    let storage = MemoryStorage::new();
    let validator = MlsValidator;

    let room_id = 100;
    let sender_id = 200;

    // Initialize room
    let state = create_test_state(room_id, 0, vec![sender_id]);
    storage.store_mls_state(room_id, &state).expect("store_mls_state failed");

    // Sequencer 1: Process 5 frames
    {
        let mut sequencer = Sequencer::new();

        for i in 0..5 {
            let frame = create_test_frame(room_id, sender_id, 0, &format!("msg-{}", i));
            let actions =
                sequencer.process_frame(frame, &storage, &validator).expect("process_frame failed");

            for action in actions {
                if let SequencerAction::StoreFrame { room_id, log_index, frame } = action {
                    storage.store_frame(room_id, log_index, &frame).expect("store_frame failed");
                }
            }
        }
    } // Sequencer dropped (simulates crash)

    // Sequencer 2: Recover and continue
    {
        let mut sequencer = Sequencer::new();

        for i in 5..10 {
            let frame = create_test_frame(room_id, sender_id, 0, &format!("msg-{}", i));
            let actions =
                sequencer.process_frame(frame, &storage, &validator).expect("process_frame failed");

            for action in actions {
                if let SequencerAction::StoreFrame { room_id, log_index, frame } = action {
                    storage.store_frame(room_id, log_index, &frame).expect("store_frame failed");
                }
            }
        }
    }

    // Oracle: Verify no gaps across restart
    verify_sequential_indices(&storage, room_id, 10);

    // Oracle: Verify monotonic log indices
    let frames = storage.load_frames(room_id, 0, 20).expect("load_frames failed");
    for window in frames.windows(2) {
        let prev = window[0].header.log_index();
        let next = window[1].header.log_index();
        assert_eq!(next, prev + 1, "non-monotonic sequence: {} -> {}", prev, next);
    }
}

#[test]
fn test_multiple_rooms_isolation() {
    let mut sequencer = Sequencer::new();
    let storage = MemoryStorage::new();
    let validator = MlsValidator;

    // Initialize two rooms
    for room_id in [100, 200] {
        let state = create_test_state(room_id, 0, vec![300]);
        storage.store_mls_state(room_id, &state).expect("store_mls_state failed");
    }

    // Send frames to both rooms in interleaved order
    for i in 0..10 {
        let room_id = if i % 2 == 0 { 100 } else { 200 };
        let frame = create_test_frame(room_id, 300, 0, &format!("room{}-{}", room_id, i));
        let actions =
            sequencer.process_frame(frame, &storage, &validator).expect("process_frame failed");

        for action in actions {
            if let SequencerAction::StoreFrame { room_id, log_index, frame } = action {
                storage.store_frame(room_id, log_index, &frame).expect("store_frame failed");
            }
        }
    }

    // Oracle: Each room has independent sequential indices
    verify_sequential_indices(&storage, 100, 5); // Room 100 got frames 0,2,4,6,8
    verify_sequential_indices(&storage, 200, 5); // Room 200 got frames 1,3,5,7,9

    // Oracle: Verify payloads are room-specific
    let room100_frames = storage.load_frames(100, 0, 10).expect("load_frames failed");
    for frame in room100_frames {
        let payload = String::from_utf8_lossy(&frame.payload);
        assert!(payload.contains("room100"), "payload: {}", payload);
    }

    let room200_frames = storage.load_frames(200, 0, 10).expect("load_frames failed");
    for frame in room200_frames {
        let payload = String::from_utf8_lossy(&frame.payload);
        assert!(payload.contains("room200"), "payload: {}", payload);
    }
}

#[test]
fn test_non_member_rejection_total_ordering_preserved() {
    let mut sequencer = Sequencer::new();
    let storage = MemoryStorage::new();
    let validator = MlsValidator;

    let room_id = 100;
    let member = 200;
    let non_member = 999;

    // Initialize room with single member
    let state = create_test_state(room_id, 0, vec![member]);
    storage.store_mls_state(room_id, &state).expect("store_mls_state failed");

    // Interleave valid and invalid frames
    let frames = vec![
        (member, true),      // Accept
        (non_member, false), // Reject
        (member, true),      // Accept
        (non_member, false), // Reject
        (member, true),      // Accept
    ];

    let mut accepted_count = 0;
    let mut rejected_count = 0;

    for (sender, should_accept) in frames {
        let frame = create_test_frame(room_id, sender, 0, &format!("sender-{}", sender));
        let actions =
            sequencer.process_frame(frame, &storage, &validator).expect("process_frame failed");

        match &actions[0] {
            SequencerAction::AcceptFrame { .. } => {
                assert!(should_accept, "frame from {} should be rejected", sender);
                accepted_count += 1;

                // Execute StoreFrame
                for action in actions {
                    if let SequencerAction::StoreFrame { room_id, log_index, frame } = action {
                        storage
                            .store_frame(room_id, log_index, &frame)
                            .expect("store_frame failed");
                    }
                }
            },
            SequencerAction::RejectFrame { reason, .. } => {
                assert!(!should_accept, "frame from {} should be accepted", sender);
                assert!(reason.contains("not in group"), "unexpected reason: {}", reason);
                rejected_count += 1;
            },
            _ => panic!("unexpected action: {:?}", actions[0]),
        }
    }

    // Oracle: Verify rejection counts
    assert_eq!(accepted_count, 3);
    assert_eq!(rejected_count, 2);

    // Oracle: Verify only accepted frames are stored with sequential indices
    verify_sequential_indices(&storage, room_id, 3);

    // Oracle: Verify all stored frames are from the member
    let frames = storage.load_frames(room_id, 0, 10).expect("load_frames failed");
    for frame in frames {
        assert_eq!(frame.header.sender_id(), member, "non-member frame was stored!");
    }
}
