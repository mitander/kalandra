//! Integration tests for the Sequencer with Oracle checks
//!
//! These tests verify total ordering invariants under various scenarios:
//! - Single client sequencing
//! - Concurrent clients
//! - Crash recovery
//!
//! # Architecture Note
//!
//! MLS validation (epoch, membership) is now handled by RoomManager.
//! Sequencer focuses solely on log index assignment and total ordering.
//!
//! # Oracle Pattern
//!
//! Each test ends with an Oracle function that verifies global consistency:
//! - No gaps in log indices
//! - Monotonic ordering

use bytes::Bytes;
use lockframe_proto::{Frame, FrameHeader, Opcode};
use lockframe_server::{MemoryStorage, Sequencer, SequencerAction, Storage};

/// Helper: Create a test frame
fn create_test_frame(room_id: u128, sender_id: u64, epoch: u64, payload: &str) -> Frame {
    let mut header = FrameHeader::new(Opcode::AppMessage);
    header.set_room_id(room_id);
    header.set_sender_id(sender_id);
    header.set_epoch(epoch);

    Frame::new(header, Bytes::from(payload.to_string()))
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

    let room_id = 100;
    let sender_id = 200;

    // Send 5 frames from single client
    // Note: MLS validation is now done by RoomManager, not Sequencer
    for i in 0..5 {
        let frame = create_test_frame(room_id, sender_id, 0, &format!("msg-{}", i));
        let actions = sequencer.process_frame(frame, &storage).expect("process_frame failed");

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

    let room_id = 100;
    let client_a = 200;
    let client_b = 300;

    // Interleave frames from two clients
    // Note: MLS validation (membership check) is now done by RoomManager
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
        let actions = sequencer.process_frame(frame, &storage).expect("process_frame failed");

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
fn test_mixed_epochs_sequencing() {
    // Note: Epoch validation is now done by RoomManager.
    // Sequencer accepts frames regardless of epoch and assigns sequential log
    // indices. This test verifies Sequencer maintains total ordering across
    // epoch boundaries.
    let mut sequencer = Sequencer::new();
    let storage = MemoryStorage::new();

    let room_id = 100;
    let sender_id = 200;

    // Send frames with different epochs (Sequencer doesn't validate epochs)
    let epochs = [0, 0, 0, 1, 1, 2];
    for (i, &epoch) in epochs.iter().enumerate() {
        let frame =
            create_test_frame(room_id, sender_id, epoch, &format!("msg-{}-epoch{}", i, epoch));
        let actions = sequencer.process_frame(frame, &storage).expect("process_frame failed");

        for action in actions {
            if let SequencerAction::StoreFrame { room_id, log_index, frame } = action {
                storage.store_frame(room_id, log_index, &frame).expect("store_frame failed");
            }
        }
    }

    // Oracle: Verify sequential indices despite mixed epochs
    verify_sequential_indices(&storage, room_id, 6);

    // Oracle: Verify epochs are preserved in stored frames
    let frames = storage.load_frames(room_id, 0, 10).expect("load_frames failed");
    let stored_epochs: Vec<u64> = frames.iter().map(|f| f.header.epoch()).collect();
    assert_eq!(stored_epochs, vec![0, 0, 0, 1, 1, 2]);
}

#[test]
fn test_sequencer_restart() {
    let storage = MemoryStorage::new();

    let room_id = 100;
    let sender_id = 200;

    // Sequencer 1: Process 5 frames
    {
        let mut sequencer = Sequencer::new();

        for i in 0..5 {
            let frame = create_test_frame(room_id, sender_id, 0, &format!("msg-{}", i));
            let actions = sequencer.process_frame(frame, &storage).expect("process_frame failed");

            for action in actions {
                if let SequencerAction::StoreFrame { room_id, log_index, frame } = action {
                    storage.store_frame(room_id, log_index, &frame).expect("store_frame failed");
                }
            }
        }
    } // Sequencer dropped (simulates crash)

    // Sequencer 2: Recover and continue
    // A new Sequencer loads next_log_index from storage, so ordering continues
    {
        let mut sequencer = Sequencer::new();

        for i in 5..10 {
            let frame = create_test_frame(room_id, sender_id, 0, &format!("msg-{}", i));
            let actions = sequencer.process_frame(frame, &storage).expect("process_frame failed");

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

    // Send frames to both rooms in interleaved order
    // Note: Room initialization/membership validation is handled by RoomManager
    for i in 0..10 {
        let room_id = if i % 2 == 0 { 100 } else { 200 };
        let frame = create_test_frame(room_id, 300, 0, &format!("room{}-{}", room_id, i));
        let actions = sequencer.process_frame(frame, &storage).expect("process_frame failed");

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
fn test_sequencer_accepts_all_senders() {
    // Note: Membership validation is now done by RoomManager.
    // Sequencer accepts ALL frames and assigns sequential log indices.
    // This test verifies Sequencer doesn't discriminate based on sender_id.
    let mut sequencer = Sequencer::new();
    let storage = MemoryStorage::new();

    let room_id = 100;
    let senders = [200, 300, 400, 500, 600];

    // Send frames from multiple senders
    for (i, &sender) in senders.iter().enumerate() {
        let frame = create_test_frame(room_id, sender, 0, &format!("sender-{}", sender));
        let actions = sequencer.process_frame(frame, &storage).expect("process_frame failed");

        // All frames should be accepted
        match &actions[0] {
            SequencerAction::AcceptFrame { log_index, .. } => {
                assert_eq!(*log_index, i as u64, "wrong log_index for sender {}", sender);
            },
            other => panic!("expected AcceptFrame, got: {:?}", other),
        }

        // Execute StoreFrame
        for action in actions {
            if let SequencerAction::StoreFrame { room_id, log_index, frame } = action {
                storage.store_frame(room_id, log_index, &frame).expect("store_frame failed");
            }
        }
    }

    // Oracle: All frames stored with sequential indices
    verify_sequential_indices(&storage, room_id, 5);

    // Oracle: Verify all senders are represented
    let frames = storage.load_frames(room_id, 0, 10).expect("load_frames failed");
    let stored_senders: Vec<u64> = frames.iter().map(|f| f.header.sender_id()).collect();
    assert_eq!(stored_senders, vec![200, 300, 400, 500, 600]);
}
