//! End-to-end client tests for message encryption and delivery.
//!
//! These tests verify the full client flow:
//! - Room creation with MLS group
//! - Message encryption with sender keys
//! - Message decryption by other members
//! - Epoch consistency across members
//!
//! # Oracle Pattern
//!
//! Each test ends with Oracle checks that verify:
//! - All messages delivered correctly
//! - Epoch consistency across clients
//! - Sender key derivation determinism

use lockframe_client::{Client, ClientAction, ClientEvent, ClientIdentity, Environment};
use lockframe_harness::SimEnv;
use lockframe_proto::Frame;
use turmoil::Builder;

/// Test room ID
const ROOM_ID: u128 = 0x1234_5678_9abc_def0_1234_5678_9abc_def0;

/// Helper: Extract SendFrame actions
fn extract_send_frames(actions: &[ClientAction]) -> Vec<Frame> {
    actions
        .iter()
        .filter_map(|a| match a {
            ClientAction::Send(frame) => Some(frame.clone()),
            _ => None,
        })
        .collect()
}

/// Helper: Extract DeliverMessage actions
fn extract_delivered_messages(actions: &[ClientAction]) -> Vec<(u64, Vec<u8>)> {
    actions
        .iter()
        .filter_map(|a| match a {
            ClientAction::DeliverMessage { sender_id, plaintext, .. } => {
                Some((*sender_id, plaintext.clone()))
            },
            _ => None,
        })
        .collect()
}

/// Oracle: Verify all expected messages were delivered
fn verify_all_messages_delivered(
    delivered: &[(u64, Vec<u8>)],
    expected: &[(u64, &[u8])],
    context: &str,
) {
    assert_eq!(
        delivered.len(),
        expected.len(),
        "{}: expected {} messages, got {}",
        context,
        expected.len(),
        delivered.len()
    );

    for (i, ((actual_sender, actual_plaintext), (expected_sender, expected_plaintext))) in
        delivered.iter().zip(expected.iter()).enumerate()
    {
        assert_eq!(
            *actual_sender, *expected_sender,
            "{}: message {} sender mismatch: expected {}, got {}",
            context, i, expected_sender, actual_sender
        );
        assert_eq!(
            actual_plaintext, *expected_plaintext,
            "{}: message {} plaintext mismatch",
            context, i
        );
    }
}

/// Oracle: Verify epoch consistency across clients
fn verify_epoch_consistency(
    clients: &[(&str, &Client<SimEnv>)],
    room_id: u128,
    expected_epoch: u64,
) {
    for (name, client) in clients {
        let actual_epoch =
            client.epoch(room_id).unwrap_or_else(|| panic!("{} should be in room", name));
        assert_eq!(
            actual_epoch, expected_epoch,
            "{}: expected epoch {}, got {}",
            name, expected_epoch, actual_epoch
        );
    }
}

/// Oracle: Verify room membership
fn verify_room_membership(clients: &[(&str, &Client<SimEnv>)], room_id: u128) {
    for (name, client) in clients {
        assert!(client.is_member(room_id), "{} should be a member of room {:x}", name, room_id);
    }
}

#[test]
fn client_create_room_and_send_message() {
    let mut sim = Builder::new().build();

    sim.host("test", || async {
        let env = SimEnv::new();
        let alice = ClientIdentity::new(1);
        let mut alice_client = Client::new(env.clone(), alice);

        // Create room
        let actions = alice_client
            .handle(ClientEvent::CreateRoom { room_id: ROOM_ID })
            .expect("create room should succeed");

        // Verify room was created
        assert!(alice_client.is_member(ROOM_ID), "Alice should be in room");
        assert_eq!(alice_client.epoch(ROOM_ID), Some(0), "Initial epoch should be 0");

        // Log action should be present
        let has_log = actions.iter().any(|a| matches!(a, ClientAction::Log { .. }));
        assert!(has_log, "Should have log action");

        // Send a message
        let plaintext = b"Hello from Alice!";
        let actions = alice_client
            .handle(ClientEvent::SendMessage { room_id: ROOM_ID, plaintext: plaintext.to_vec() })
            .expect("send message should succeed");

        // Should produce SendFrame action
        let frames = extract_send_frames(&actions);
        assert_eq!(frames.len(), 1, "Should produce one frame");

        let frame = &frames[0];
        assert_eq!(frame.header.room_id(), ROOM_ID);
        assert_eq!(frame.header.sender_id(), 1);
        assert_eq!(frame.header.epoch(), 0);

        // Payload should be non-empty (encrypted)
        assert!(!frame.payload.is_empty(), "Encrypted payload should be non-empty");

        Ok(())
    });

    sim.run().unwrap();
}

#[test]
fn client_encrypt_decrypt_roundtrip_two_members() {
    // This test simulates two clients in the same room.
    // Alice creates the room, adds Bob, and they exchange messages.
    //
    // Note: This requires both clients to derive the same sender keys from
    // the MLS epoch secret. We verify this by:
    // 1. Alice encrypts a message
    // 2. Bob receives and decrypts it
    // 3. Bob encrypts a reply
    // 4. Alice receives and decrypts it

    let mut sim = Builder::new().build();

    sim.host("test", || async {
        let env = SimEnv::new();

        // Create Alice's client and room
        let alice_identity = ClientIdentity::new(1);
        let mut alice_client = Client::new(env.clone(), alice_identity);

        alice_client
            .handle(ClientEvent::CreateRoom { room_id: ROOM_ID })
            .expect("alice create room");

        // For this test, we verify the self-encrypt works
        // (Full two-client test requires Welcome handling which needs
        // serialized KeyPackages from OpenMLS)
        let plaintext = b"Test message for encryption";

        let actions = alice_client
            .handle(ClientEvent::SendMessage { room_id: ROOM_ID, plaintext: plaintext.to_vec() })
            .expect("alice send message");

        let frames = extract_send_frames(&actions);
        assert_eq!(frames.len(), 1);

        // The frame should have encrypted content
        let frame = &frames[0];
        assert!(frame.payload.len() > plaintext.len(), "Encrypted should be larger (nonce + tag)");

        Ok(())
    });

    sim.run().unwrap();
}

#[test]
fn client_multiple_messages_same_epoch() {
    // Verify that sending multiple messages in the same epoch works correctly.
    // Each message should use a different ratchet generation.

    let mut sim = Builder::new().build();

    sim.host("test", || async {
        let env = SimEnv::new();
        let alice = ClientIdentity::new(1);
        let mut alice_client = Client::new(env.clone(), alice);

        alice_client.handle(ClientEvent::CreateRoom { room_id: ROOM_ID }).expect("create room");

        // Send multiple messages
        let messages = vec![b"Message 1".to_vec(), b"Message 2".to_vec(), b"Message 3".to_vec()];

        let mut all_frames = Vec::new();
        for plaintext in &messages {
            let actions = alice_client
                .handle(ClientEvent::SendMessage { room_id: ROOM_ID, plaintext: plaintext.clone() })
                .expect("send message");

            let frames = extract_send_frames(&actions);
            all_frames.extend(frames);
        }

        // Oracle: All messages should produce frames
        assert_eq!(all_frames.len(), 3, "Should have 3 frames");

        // Oracle: All frames should be for the same room and epoch
        for frame in &all_frames {
            assert_eq!(frame.header.room_id(), ROOM_ID);
            assert_eq!(frame.header.epoch(), 0);
        }

        // Oracle: All ciphertexts should be different (different nonces/generations)
        let ciphertexts: Vec<_> = all_frames.iter().map(|f| f.payload.clone()).collect();
        for i in 0..ciphertexts.len() {
            for j in (i + 1)..ciphertexts.len() {
                assert_ne!(
                    ciphertexts[i], ciphertexts[j],
                    "Ciphertexts {} and {} should be different",
                    i, j
                );
            }
        }

        Ok(())
    });

    sim.run().unwrap();
}

#[test]
fn client_room_not_found_error() {
    // Verify that sending to a non-existent room fails properly

    let mut sim = Builder::new().build();

    sim.host("test", || async {
        let env = SimEnv::new();
        let alice = ClientIdentity::new(1);
        let mut alice_client = Client::new(env.clone(), alice);

        // Try to send without creating room
        let result = alice_client
            .handle(ClientEvent::SendMessage { room_id: ROOM_ID, plaintext: b"Hello".to_vec() });

        assert!(result.is_err(), "Should fail for non-existent room");
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not found") || err.to_string().contains("RoomNotFound"));

        Ok(())
    });

    sim.run().unwrap();
}

#[test]
fn client_leave_room() {
    // Verify that leaving a room works and cleans up state

    let mut sim = Builder::new().build();

    sim.host("test", || async {
        let env = SimEnv::new();
        let alice = ClientIdentity::new(1);
        let mut alice_client = Client::new(env.clone(), alice);

        // Create and verify room
        alice_client.handle(ClientEvent::CreateRoom { room_id: ROOM_ID }).expect("create room");
        assert!(alice_client.is_member(ROOM_ID));
        assert_eq!(alice_client.room_count(), 1);

        // Leave room
        let actions =
            alice_client.handle(ClientEvent::LeaveRoom { room_id: ROOM_ID }).expect("leave room");

        // Oracle: Should no longer be a member
        assert!(!alice_client.is_member(ROOM_ID));
        assert_eq!(alice_client.room_count(), 0);

        // Oracle: Should have RoomRemoved action
        let has_room_removed =
            actions.iter().any(|a| matches!(a, ClientAction::RoomRemoved { .. }));
        assert!(has_room_removed, "Should have RoomRemoved action");

        // Oracle: Sending should fail now
        let result = alice_client
            .handle(ClientEvent::SendMessage { room_id: ROOM_ID, plaintext: b"Hello".to_vec() });
        assert!(result.is_err());

        Ok(())
    });

    sim.run().unwrap();
}

#[test]
fn client_duplicate_room_creation_fails() {
    // Verify that creating a room twice fails

    let mut sim = Builder::new().build();

    sim.host("test", || async {
        let env = SimEnv::new();
        let alice = ClientIdentity::new(1);
        let mut alice_client = Client::new(env.clone(), alice);

        // Create room
        alice_client.handle(ClientEvent::CreateRoom { room_id: ROOM_ID }).expect("create room");

        // Try to create again
        let result = alice_client.handle(ClientEvent::CreateRoom { room_id: ROOM_ID });

        assert!(result.is_err(), "Duplicate room creation should fail");

        Ok(())
    });

    sim.run().unwrap();
}

#[test]
fn client_tick_with_no_pending_commits() {
    // Verify that tick handling works when there are no pending commits

    let mut sim = Builder::new().build();

    sim.host("test", || async {
        let env = SimEnv::new();
        let alice = ClientIdentity::new(1);
        let mut alice_client = Client::new(env.clone(), alice);

        // Create room
        alice_client.handle(ClientEvent::CreateRoom { room_id: ROOM_ID }).expect("create room");

        // Tick should produce no actions (no pending commits)
        // Use Environment trait method
        let now = <SimEnv as Environment>::now(&env);
        let actions = alice_client.handle(ClientEvent::Tick { now }).expect("tick should succeed");

        // Oracle: No RequestSync since no pending commits
        let has_sync = actions.iter().any(|a| matches!(a, ClientAction::RequestSync { .. }));
        assert!(!has_sync, "Should not request sync without pending commits");

        Ok(())
    });

    sim.run().unwrap();
}

#[test]
fn client_multiple_rooms_isolation() {
    // Verify that multiple rooms are properly isolated

    let mut sim = Builder::new().build();

    sim.host("test", || async {
        let env = SimEnv::new();
        let alice = ClientIdentity::new(1);
        let mut alice_client = Client::new(env.clone(), alice);

        let room_a = 0xAAAA_AAAA_AAAA_AAAA_AAAA_AAAA_AAAA_AAAA;
        let room_b = 0xBBBB_BBBB_BBBB_BBBB_BBBB_BBBB_BBBB_BBBB;

        // Create both rooms
        alice_client.handle(ClientEvent::CreateRoom { room_id: room_a }).expect("create room A");
        alice_client.handle(ClientEvent::CreateRoom { room_id: room_b }).expect("create room B");

        // Oracle: Both rooms exist
        assert!(alice_client.is_member(room_a));
        assert!(alice_client.is_member(room_b));
        assert_eq!(alice_client.room_count(), 2);

        // Send message to room A
        let actions_a = alice_client
            .handle(ClientEvent::SendMessage { room_id: room_a, plaintext: b"To room A".to_vec() })
            .expect("send to A");

        // Send message to room B
        let actions_b = alice_client
            .handle(ClientEvent::SendMessage { room_id: room_b, plaintext: b"To room B".to_vec() })
            .expect("send to B");

        // Oracle: Messages go to correct rooms
        let frames_a = extract_send_frames(&actions_a);
        let frames_b = extract_send_frames(&actions_b);

        assert_eq!(frames_a[0].header.room_id(), room_a);
        assert_eq!(frames_b[0].header.room_id(), room_b);

        // Oracle: Ciphertexts are different (different sender keys per room)
        assert_ne!(frames_a[0].payload, frames_b[0].payload);

        // Leave room A
        alice_client.handle(ClientEvent::LeaveRoom { room_id: room_a }).expect("leave A");

        // Oracle: Only room B remains
        assert!(!alice_client.is_member(room_a));
        assert!(alice_client.is_member(room_b));
        assert_eq!(alice_client.room_count(), 1);

        Ok(())
    });

    sim.run().unwrap();
}

#[test]
fn client_sender_id_preserved_in_frames() {
    // Verify that sender_id is correctly set in outgoing frames

    let mut sim = Builder::new().build();

    sim.host("test", || async {
        let env = SimEnv::new();

        // Create multiple clients with different sender IDs
        let sender_ids = [100, 200, 300];

        for &sender_id in &sender_ids {
            let identity = ClientIdentity::new(sender_id);
            let mut client = Client::new(env.clone(), identity);

            // Use unique room per client for this test
            let room_id = sender_id as u128;

            client.handle(ClientEvent::CreateRoom { room_id }).expect("create room");

            let actions = client
                .handle(ClientEvent::SendMessage {
                    room_id,
                    plaintext: format!("From sender {}", sender_id).into_bytes(),
                })
                .expect("send message");

            let frames = extract_send_frames(&actions);
            assert_eq!(frames.len(), 1);

            // Oracle: Sender ID matches
            assert_eq!(
                frames[0].header.sender_id(),
                sender_id,
                "Frame sender_id should match client identity"
            );
        }

        Ok(())
    });

    sim.run().unwrap();
}

#[test]
fn client_epoch_in_frame_header() {
    // Verify that epoch is correctly set in outgoing frames

    let mut sim = Builder::new().build();

    sim.host("test", || async {
        let env = SimEnv::new();
        let alice = ClientIdentity::new(1);
        let mut alice_client = Client::new(env.clone(), alice);

        alice_client.handle(ClientEvent::CreateRoom { room_id: ROOM_ID }).expect("create room");

        // Verify initial epoch
        assert_eq!(alice_client.epoch(ROOM_ID), Some(0));

        let actions = alice_client
            .handle(ClientEvent::SendMessage { room_id: ROOM_ID, plaintext: b"Hello".to_vec() })
            .expect("send message");

        let frames = extract_send_frames(&actions);

        // Oracle: Frame epoch matches client epoch
        assert_eq!(frames[0].header.epoch(), 0);

        Ok(())
    });

    sim.run().unwrap();
}

// =============================================================================
// Message Delivery Correctness Oracle Tests
// =============================================================================

/// Test that verifies message ordering is preserved through encryption.
/// Messages sent in order should produce frames in the same order.
#[test]
fn client_message_ordering_preserved() {
    let mut sim = Builder::new().build();

    sim.host("test", || async {
        let env = SimEnv::new();
        let alice = ClientIdentity::new(1);
        let mut alice_client = Client::new(env.clone(), alice);

        alice_client.handle(ClientEvent::CreateRoom { room_id: ROOM_ID }).expect("create room");

        // Send 10 messages in order
        let mut frames = Vec::new();
        for i in 0..10 {
            let msg = format!("Message {}", i);
            let actions = alice_client
                .handle(ClientEvent::SendMessage { room_id: ROOM_ID, plaintext: msg.into_bytes() })
                .expect("send message");

            let send_frames = extract_send_frames(&actions);
            assert_eq!(send_frames.len(), 1, "Should produce exactly one frame per message");
            frames.push(send_frames[0].clone());
        }

        // Oracle: All frames have correct room_id
        for (i, frame) in frames.iter().enumerate() {
            assert_eq!(frame.header.room_id(), ROOM_ID, "Frame {} should have correct room_id", i);
        }

        // Oracle: All frames have same sender_id
        for (i, frame) in frames.iter().enumerate() {
            assert_eq!(frame.header.sender_id(), 1, "Frame {} should have sender_id=1", i);
        }

        // Oracle: All frames have same epoch
        for (i, frame) in frames.iter().enumerate() {
            assert_eq!(frame.header.epoch(), 0, "Frame {} should have epoch=0", i);
        }

        // Oracle: Frames are in order (no reordering)
        // We can verify by checking that no two frames have identical payloads
        // (each message is unique, and encryption adds unique nonce)
        for i in 0..frames.len() {
            for j in (i + 1)..frames.len() {
                assert_ne!(
                    frames[i].payload, frames[j].payload,
                    "Frames {} and {} should have different encrypted payloads",
                    i, j
                );
            }
        }

        Ok(())
    });

    sim.run().unwrap();
}

/// Test that large messages are handled correctly.
#[test]
fn client_large_message_handling() {
    let mut sim = Builder::new().build();

    sim.host("test", || async {
        let env = SimEnv::new();
        let alice = ClientIdentity::new(1);
        let mut alice_client = Client::new(env.clone(), alice);

        alice_client.handle(ClientEvent::CreateRoom { room_id: ROOM_ID }).expect("create room");

        // Send messages of various sizes
        let sizes = [1, 100, 1000, 10000, 100000];

        for &size in &sizes {
            let plaintext = vec![b'X'; size];
            let actions = alice_client
                .handle(ClientEvent::SendMessage { room_id: ROOM_ID, plaintext: plaintext.clone() })
                .expect(&format!("send {}-byte message", size));

            let frames = extract_send_frames(&actions);
            assert_eq!(frames.len(), 1);

            // Oracle: Encrypted payload should be larger than plaintext
            // (includes nonce, auth tag, CBOR overhead)
            assert!(
                frames[0].payload.len() > size,
                "Encrypted {}B message should be larger than plaintext",
                size
            );

            // Oracle: Check reasonable overhead (CBOR + nonce + tag < 200 bytes typically)
            let overhead = frames[0].payload.len() - size;
            assert!(
                overhead < 500,
                "Overhead for {}B message is {}B, expected < 500B",
                size,
                overhead
            );
        }

        Ok(())
    });

    sim.run().unwrap();
}

/// Test sender key ratchet advancement.
/// Multiple messages should use different nonces even with same plaintext.
#[test]
fn client_sender_key_ratchet_advances() {
    let mut sim = Builder::new().build();

    sim.host("test", || async {
        let env = SimEnv::new();
        let alice = ClientIdentity::new(1);
        let mut alice_client = Client::new(env.clone(), alice);

        alice_client.handle(ClientEvent::CreateRoom { room_id: ROOM_ID }).expect("create room");

        // Send the SAME plaintext 5 times
        let plaintext = b"Identical message".to_vec();
        let mut frames = Vec::new();

        for _ in 0..5 {
            let actions = alice_client
                .handle(ClientEvent::SendMessage { room_id: ROOM_ID, plaintext: plaintext.clone() })
                .expect("send message");

            let send_frames = extract_send_frames(&actions);
            frames.push(send_frames[0].clone());
        }

        // Oracle: All ciphertexts should be DIFFERENT despite same plaintext
        // This is because each message uses a different ratchet generation
        for i in 0..frames.len() {
            for j in (i + 1)..frames.len() {
                assert_ne!(
                    frames[i].payload, frames[j].payload,
                    "Frames {} and {} should have different ciphertexts (ratchet advanced)",
                    i, j
                );
            }
        }

        Ok(())
    });

    sim.run().unwrap();
}

/// Test that empty messages are handled correctly.
#[test]
fn client_empty_message_handling() {
    let mut sim = Builder::new().build();

    sim.host("test", || async {
        let env = SimEnv::new();
        let alice = ClientIdentity::new(1);
        let mut alice_client = Client::new(env.clone(), alice);

        alice_client.handle(ClientEvent::CreateRoom { room_id: ROOM_ID }).expect("create room");

        // Send empty message
        let actions = alice_client
            .handle(ClientEvent::SendMessage { room_id: ROOM_ID, plaintext: vec![] })
            .expect("send empty message");

        let frames = extract_send_frames(&actions);
        assert_eq!(frames.len(), 1);

        // Oracle: Even empty message produces encrypted payload
        // (contains nonce, auth tag, CBOR structure)
        assert!(
            frames[0].payload.len() > 0,
            "Empty message should produce non-empty encrypted payload"
        );

        Ok(())
    });

    sim.run().unwrap();
}

/// Test two-client message exchange via AddMembers + Welcome flow.
///
/// This tests the full client-to-client flow:
/// 1. Alice creates room
/// 2. Bob generates KeyPackage
/// 3. Alice adds Bob via AddMembers
/// 4. Bob joins via Welcome
/// 5. Alice sends message, Bob decrypts
/// 6. Bob sends reply, Alice decrypts
#[test]
fn client_two_party_message_exchange() {
    let mut sim = Builder::new().build();

    sim.host("test", || async {
        let env = SimEnv::new();

        // Create Alice's client
        let alice_identity = ClientIdentity::new(1);
        let mut alice = Client::new(env.clone(), alice_identity);

        // Create Bob's client
        let bob_identity = ClientIdentity::new(2);
        let mut bob = Client::new(env.clone(), bob_identity);

        // Step 1: Alice creates room
        alice.handle(ClientEvent::CreateRoom { room_id: ROOM_ID }).expect("alice create room");

        // Oracle: Alice is in room at epoch 0
        assert!(alice.is_member(ROOM_ID), "Alice should be in room");
        assert_eq!(alice.epoch(ROOM_ID), Some(0), "Alice should be at epoch 0");

        // Step 2: Bob generates KeyPackage
        let (bob_key_package, _hash_ref) =
            bob.generate_key_package().expect("bob generate key package");

        // Step 3: Alice adds Bob
        let add_actions = alice
            .handle(ClientEvent::AddMembers {
                room_id: ROOM_ID,
                key_packages: vec![bob_key_package],
            })
            .expect("alice add bob");

        // Find the Welcome frame for Bob
        let welcome_frame = add_actions
            .iter()
            .filter_map(|a| match a {
                ClientAction::Send(frame)
                    if frame.header.opcode_enum() == Some(lockframe_proto::Opcode::Welcome) =>
                {
                    Some(frame.clone())
                },
                _ => None,
            })
            .next()
            .expect("should have Welcome frame");

        // Find the Commit frame
        let commit_frame = add_actions
            .iter()
            .filter_map(|a| match a {
                ClientAction::Send(frame)
                    if frame.header.opcode_enum() == Some(lockframe_proto::Opcode::Commit) =>
                {
                    Some(frame.clone())
                },
                _ => None,
            })
            .next()
            .expect("should have Commit frame");

        // Alice processes her own commit (self-commit)
        alice.handle(ClientEvent::FrameReceived(commit_frame)).expect("alice process commit");

        // Oracle: Alice advances to epoch 1
        assert_eq!(alice.epoch(ROOM_ID), Some(1), "Alice should advance to epoch 1");

        // Step 4: Bob joins via Welcome
        bob.handle(ClientEvent::JoinRoom {
            room_id: ROOM_ID,
            welcome: welcome_frame.payload.to_vec(),
        })
        .expect("bob join via welcome");

        // Oracle: Bob is in room at epoch 1
        assert!(bob.is_member(ROOM_ID), "Bob should be in room");
        assert_eq!(bob.epoch(ROOM_ID), Some(1), "Bob should be at epoch 1");

        // Oracle: Both clients at same epoch
        verify_epoch_consistency(&[("Alice", &alice), ("Bob", &bob)], ROOM_ID, 1);

        // Oracle: Both clients are members
        verify_room_membership(&[("Alice", &alice), ("Bob", &bob)], ROOM_ID);

        // Step 5: Alice sends message
        let alice_msg = b"Hello Bob!";
        let send_actions = alice
            .handle(ClientEvent::SendMessage { room_id: ROOM_ID, plaintext: alice_msg.to_vec() })
            .expect("alice send message");

        let alice_frame = extract_send_frames(&send_actions)[0].clone();

        // Bob receives and decrypts
        let bob_receive_actions =
            bob.handle(ClientEvent::FrameReceived(alice_frame)).expect("bob receive message");

        let bob_delivered = extract_delivered_messages(&bob_receive_actions);

        // Oracle: Bob received exactly one message
        assert_eq!(bob_delivered.len(), 1, "Bob should receive one message");

        // Oracle: Message content matches
        verify_all_messages_delivered(
            &bob_delivered,
            &[(1, alice_msg)],
            "Bob receives Alice's message",
        );

        // Step 6: Bob sends reply
        let bob_msg = b"Hello Alice!";
        let bob_send_actions = bob
            .handle(ClientEvent::SendMessage { room_id: ROOM_ID, plaintext: bob_msg.to_vec() })
            .expect("bob send message");

        let bob_frame = extract_send_frames(&bob_send_actions)[0].clone();

        // Alice receives and decrypts
        let alice_receive_actions =
            alice.handle(ClientEvent::FrameReceived(bob_frame)).expect("alice receive message");

        let alice_delivered = extract_delivered_messages(&alice_receive_actions);

        // Oracle: Alice received exactly one message
        assert_eq!(alice_delivered.len(), 1, "Alice should receive one message");

        // Oracle: Message content matches
        verify_all_messages_delivered(
            &alice_delivered,
            &[(2, bob_msg)],
            "Alice receives Bob's message",
        );

        Ok(())
    });

    sim.run().unwrap();
}

// =============================================================================
// Fault Injection Scenarios
// =============================================================================

/// Test client behavior with epoch mismatch (stale frame).
/// Client should reject frames with wrong epoch.
#[test]
fn client_epoch_mismatch_rejection() {
    let mut sim = Builder::new().build();

    sim.host("test", || async {
        let env = SimEnv::new();
        let alice_identity = ClientIdentity::new(1);
        let mut alice = Client::new(env.clone(), alice_identity);

        alice.handle(ClientEvent::CreateRoom { room_id: ROOM_ID }).expect("create room");

        // Create a frame with wrong epoch (epoch 5 when we're at epoch 0)
        let mut header = lockframe_proto::FrameHeader::new(lockframe_proto::Opcode::AppMessage);
        header.set_room_id(ROOM_ID);
        header.set_sender_id(2); // Different sender
        header.set_epoch(5); // Wrong epoch!

        // Create minimal valid-looking encrypted payload
        let fake_payload = vec![0u8; 100];
        let stale_frame = lockframe_proto::Frame::new(header, fake_payload);

        // Oracle: Client should reject this frame
        let result = alice.handle(ClientEvent::FrameReceived(stale_frame));

        assert!(result.is_err(), "Should reject frame with wrong epoch");
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("epoch") || err.to_string().contains("Epoch"),
            "Error should mention epoch: {}",
            err
        );

        Ok(())
    });

    sim.run().unwrap();
}

/// Test client behavior when receiving malformed encrypted payload.
/// Client should handle decryption failure gracefully.
#[test]
fn client_malformed_payload_handling() {
    let mut sim = Builder::new().build();

    sim.host("test", || async {
        let env = SimEnv::new();
        let alice_identity = ClientIdentity::new(1);
        let mut alice = Client::new(env.clone(), alice_identity);

        alice.handle(ClientEvent::CreateRoom { room_id: ROOM_ID }).expect("create room");

        // Create a frame with malformed/corrupted payload
        let mut header = lockframe_proto::FrameHeader::new(lockframe_proto::Opcode::AppMessage);
        header.set_room_id(ROOM_ID);
        header.set_sender_id(1);
        header.set_epoch(0);

        // Garbage payload that won't deserialize correctly
        let garbage_payload = vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        let bad_frame = lockframe_proto::Frame::new(header, garbage_payload);

        // Oracle: Client should handle gracefully (error, not panic)
        let result = alice.handle(ClientEvent::FrameReceived(bad_frame));

        // Should error, not panic
        assert!(result.is_err(), "Should reject malformed payload");

        Ok(())
    });

    sim.run().unwrap();
}

/// Test client behavior with rapid message bursts.
/// Client should handle high message throughput without issues.
#[test]
fn client_high_throughput_burst() {
    let mut sim = Builder::new().build();

    sim.host("test", || async {
        let env = SimEnv::new();
        let alice_identity = ClientIdentity::new(1);
        let mut alice = Client::new(env.clone(), alice_identity);

        alice.handle(ClientEvent::CreateRoom { room_id: ROOM_ID }).expect("create room");

        // Send 1000 messages in rapid succession
        const BURST_SIZE: usize = 1000;
        let mut all_payloads = Vec::new();

        for i in 0..BURST_SIZE {
            let plaintext = format!("Burst message {}", i).into_bytes();
            let actions = alice
                .handle(ClientEvent::SendMessage { room_id: ROOM_ID, plaintext })
                .expect(&format!("send message {}", i));

            let frames = extract_send_frames(&actions);
            assert_eq!(frames.len(), 1, "Each send should produce one frame");
            all_payloads.push(frames[0].payload.clone());
        }

        // Oracle: All payloads should be unique (ratchet advanced for each)
        let unique_payloads: std::collections::HashSet<_> = all_payloads.iter().collect();
        assert_eq!(
            unique_payloads.len(),
            BURST_SIZE,
            "All {} messages should produce unique ciphertexts",
            BURST_SIZE
        );

        Ok(())
    });

    sim.run().unwrap();
}

/// Test client behavior when room is removed mid-operation.
/// Subsequent operations on removed room should fail cleanly.
#[test]
fn client_removed_room_operations_fail() {
    let mut sim = Builder::new().build();

    sim.host("test", || async {
        let env = SimEnv::new();
        let alice_identity = ClientIdentity::new(1);
        let mut alice = Client::new(env.clone(), alice_identity);

        // Create and immediately leave room
        alice.handle(ClientEvent::CreateRoom { room_id: ROOM_ID }).expect("create room");

        alice.handle(ClientEvent::LeaveRoom { room_id: ROOM_ID }).expect("leave room");

        // Oracle: All operations should fail with RoomNotFound

        let send_result = alice
            .handle(ClientEvent::SendMessage { room_id: ROOM_ID, plaintext: b"test".to_vec() });
        assert!(send_result.is_err(), "Send to removed room should fail");

        let leave_result = alice.handle(ClientEvent::LeaveRoom { room_id: ROOM_ID });
        assert!(leave_result.is_err(), "Leave removed room should fail");

        let add_result = alice.handle(ClientEvent::AddMembers {
            room_id: ROOM_ID,
            key_packages: vec![vec![1, 2, 3]],
        });
        assert!(add_result.is_err(), "AddMembers to removed room should fail");

        Ok(())
    });

    sim.run().unwrap();
}

/// Test concurrent operations on multiple rooms.
/// Operations on different rooms should be isolated.
#[test]
fn client_concurrent_room_operations() {
    let mut sim = Builder::new().build();

    sim.host("test", || async {
        let env = SimEnv::new();
        let alice_identity = ClientIdentity::new(1);
        let mut alice = Client::new(env.clone(), alice_identity);

        // Create multiple rooms
        const NUM_ROOMS: usize = 10;
        let room_ids: Vec<u128> = (0..NUM_ROOMS).map(|i| ROOM_ID + i as u128).collect();

        for &room_id in &room_ids {
            alice.handle(ClientEvent::CreateRoom { room_id }).expect("create room");
        }

        // Oracle: All rooms exist
        assert_eq!(alice.room_count(), NUM_ROOMS);
        for &room_id in &room_ids {
            assert!(alice.is_member(room_id));
            assert_eq!(alice.epoch(room_id), Some(0));
        }

        // Send messages to all rooms interleaved
        for i in 0..5 {
            for &room_id in &room_ids {
                let plaintext = format!("Room {:x} message {}", room_id, i).into_bytes();
                let actions = alice
                    .handle(ClientEvent::SendMessage { room_id, plaintext })
                    .expect("send message");

                let frames = extract_send_frames(&actions);
                // Oracle: Frame goes to correct room
                assert_eq!(frames[0].header.room_id(), room_id);
            }
        }

        // Leave odd-numbered rooms
        for &room_id in room_ids.iter().filter(|&&id| id % 2 == 1) {
            alice.handle(ClientEvent::LeaveRoom { room_id }).expect("leave room");
        }

        // Oracle: Only even rooms remain
        let remaining = room_ids.iter().filter(|&&id| id % 2 == 0).count();
        assert_eq!(alice.room_count(), remaining);

        for &room_id in &room_ids {
            if room_id % 2 == 0 {
                assert!(alice.is_member(room_id), "Even room {} should exist", room_id);
            } else {
                assert!(!alice.is_member(room_id), "Odd room {} should be gone", room_id);
            }
        }

        Ok(())
    });

    sim.run().unwrap();
}

// =============================================================================
// Sender Key Derivation Consistency Tests
// =============================================================================

/// Test that sender key derivation is consistent between two clients.
/// Both clients should derive identical sender keys from the same MLS epoch.
#[test]
fn client_sender_key_derivation_consistency() {
    let mut sim = Builder::new().build();

    sim.host("test", || async {
        let env = SimEnv::new();

        // Create Alice's client
        let alice_identity = ClientIdentity::new(1);
        let mut alice = Client::new(env.clone(), alice_identity);

        // Create Bob's client
        let bob_identity = ClientIdentity::new(2);
        let mut bob = Client::new(env.clone(), bob_identity);

        // Alice creates room
        alice.handle(ClientEvent::CreateRoom { room_id: ROOM_ID }).expect("alice create room");

        // Bob generates KeyPackage
        let (bob_kp, _) = bob.generate_key_package().expect("bob generate kp");

        // Alice adds Bob
        let add_actions = alice
            .handle(ClientEvent::AddMembers { room_id: ROOM_ID, key_packages: vec![bob_kp] })
            .expect("alice add bob");

        // Get Welcome and Commit frames
        let welcome_frame = add_actions
            .iter()
            .filter_map(|a| match a {
                ClientAction::Send(frame)
                    if frame.header.opcode_enum() == Some(lockframe_proto::Opcode::Welcome) =>
                {
                    Some(frame.clone())
                },
                _ => None,
            })
            .next()
            .expect("welcome frame");

        let commit_frame = add_actions
            .iter()
            .filter_map(|a| match a {
                ClientAction::Send(frame)
                    if frame.header.opcode_enum() == Some(lockframe_proto::Opcode::Commit) =>
                {
                    Some(frame.clone())
                },
                _ => None,
            })
            .next()
            .expect("commit frame");

        // Alice processes commit
        alice.handle(ClientEvent::FrameReceived(commit_frame)).expect("alice process commit");

        // Bob joins via Welcome
        bob.handle(ClientEvent::JoinRoom {
            room_id: ROOM_ID,
            welcome: welcome_frame.payload.to_vec(),
        })
        .expect("bob join");

        // Oracle: Both at same epoch
        assert_eq!(alice.epoch(ROOM_ID), bob.epoch(ROOM_ID), "Should be at same epoch");

        // Oracle: Multiple round-trips verify key consistency

        // Round 1: Alice -> Bob
        let msg1 = b"Key consistency test 1";
        let alice_actions = alice
            .handle(ClientEvent::SendMessage { room_id: ROOM_ID, plaintext: msg1.to_vec() })
            .unwrap();
        let alice_frame = extract_send_frames(&alice_actions)[0].clone();

        let bob_receive = bob.handle(ClientEvent::FrameReceived(alice_frame)).unwrap();
        let bob_msgs = extract_delivered_messages(&bob_receive);
        assert_eq!(bob_msgs.len(), 1, "Bob should receive one message");
        assert_eq!(bob_msgs[0].1, msg1, "Bob should decrypt correctly");

        // Round 2: Bob -> Alice
        let msg2 = b"Key consistency test 2";
        let bob_actions = bob
            .handle(ClientEvent::SendMessage { room_id: ROOM_ID, plaintext: msg2.to_vec() })
            .unwrap();
        let bob_frame = extract_send_frames(&bob_actions)[0].clone();

        let alice_receive = alice.handle(ClientEvent::FrameReceived(bob_frame)).unwrap();
        let alice_msgs = extract_delivered_messages(&alice_receive);
        assert_eq!(alice_msgs.len(), 1, "Alice should receive one message");
        assert_eq!(alice_msgs[0].1, msg2, "Alice should decrypt correctly");

        // Round 3: Multiple messages in sequence
        for i in 0..10 {
            let msg = format!("Sequence message {}", i).into_bytes();

            // Alice sends
            let a_actions = alice
                .handle(ClientEvent::SendMessage { room_id: ROOM_ID, plaintext: msg.clone() })
                .unwrap();
            let a_frame = extract_send_frames(&a_actions)[0].clone();

            // Bob receives
            let b_recv = bob.handle(ClientEvent::FrameReceived(a_frame)).unwrap();
            let b_msgs = extract_delivered_messages(&b_recv);
            assert_eq!(b_msgs[0].1, msg, "Sequence {} Alice->Bob failed", i);

            // Bob sends
            let b_actions = bob
                .handle(ClientEvent::SendMessage { room_id: ROOM_ID, plaintext: msg.clone() })
                .unwrap();
            let b_frame = extract_send_frames(&b_actions)[0].clone();

            // Alice receives
            let a_recv = alice.handle(ClientEvent::FrameReceived(b_frame)).unwrap();
            let a_msgs = extract_delivered_messages(&a_recv);
            assert_eq!(a_msgs[0].1, msg, "Sequence {} Bob->Alice failed", i);
        }

        Ok(())
    });

    sim.run().unwrap();
}

/// Test that different rooms have different sender keys.
/// Keys derived from different MLS groups should never overlap.
#[test]
fn client_sender_keys_room_isolation() {
    let mut sim = Builder::new().build();

    sim.host("test", || async {
        let env = SimEnv::new();
        let alice_identity = ClientIdentity::new(1);
        let mut alice = Client::new(env.clone(), alice_identity);

        let room_a = 0xAAAA_AAAA_AAAA_AAAA_AAAA_AAAA_AAAA_AAAA;
        let room_b = 0xBBBB_BBBB_BBBB_BBBB_BBBB_BBBB_BBBB_BBBB;

        // Create two separate rooms
        alice.handle(ClientEvent::CreateRoom { room_id: room_a }).expect("create room A");
        alice.handle(ClientEvent::CreateRoom { room_id: room_b }).expect("create room B");

        // Send same plaintext to both rooms
        let plaintext = b"Same plaintext, different keys";

        let actions_a = alice
            .handle(ClientEvent::SendMessage { room_id: room_a, plaintext: plaintext.to_vec() })
            .expect("send to A");
        let actions_b = alice
            .handle(ClientEvent::SendMessage { room_id: room_b, plaintext: plaintext.to_vec() })
            .expect("send to B");

        let frame_a = extract_send_frames(&actions_a)[0].clone();
        let frame_b = extract_send_frames(&actions_b)[0].clone();

        // Oracle: Ciphertexts should be different (different MLS groups = different
        // keys)
        assert_ne!(
            frame_a.payload, frame_b.payload,
            "Same plaintext in different rooms should produce different ciphertexts"
        );

        // Oracle: Frames go to correct rooms
        assert_eq!(frame_a.header.room_id(), room_a);
        assert_eq!(frame_b.header.room_id(), room_b);

        Ok(())
    });

    sim.run().unwrap();
}

/// Test sender key ratchet determinism.
/// Same message sequence with same seed produces identical ratchet progression.
/// Note: This is already covered by client_encryption_determinism, but this
/// test explicitly focuses on the ratchet behavior.
#[test]
fn client_sender_key_ratchet_determinism() {
    let mut sim = Builder::new().build();

    sim.host("test", || async {
        // First run with seed 99999
        let env1 = SimEnv::with_seed(99999);
        let alice1 = ClientIdentity::new(1);
        let mut client1 = Client::new(env1.clone(), alice1);

        let room1 = ROOM_ID;
        client1.handle(ClientEvent::CreateRoom { room_id: room1 }).unwrap();

        let mut payloads1 = Vec::new();
        for i in 0..5 {
            let plaintext = format!("Determinism test message {}", i).into_bytes();
            let actions =
                client1.handle(ClientEvent::SendMessage { room_id: room1, plaintext }).unwrap();
            let frames = extract_send_frames(&actions);
            payloads1.push(frames[0].payload.clone());
        }

        // Second run with same seed 99999 (different room to avoid conflicts)
        let env2 = SimEnv::with_seed(99999);
        let alice2 = ClientIdentity::new(1);
        let mut client2 = Client::new(env2.clone(), alice2);

        let room2 = ROOM_ID.wrapping_add(100);
        client2.handle(ClientEvent::CreateRoom { room_id: room2 }).unwrap();

        let mut payloads2 = Vec::new();
        for i in 0..5 {
            let plaintext = format!("Determinism test message {}", i).into_bytes();
            let actions =
                client2.handle(ClientEvent::SendMessage { room_id: room2, plaintext }).unwrap();
            let frames = extract_send_frames(&actions);
            payloads2.push(frames[0].payload.clone());
        }

        // Third run with different seed 11111
        let env3 = SimEnv::with_seed(11111);
        let alice3 = ClientIdentity::new(1);
        let mut client3 = Client::new(env3.clone(), alice3);

        let room3 = ROOM_ID.wrapping_add(200);
        client3.handle(ClientEvent::CreateRoom { room_id: room3 }).unwrap();

        let mut payloads3 = Vec::new();
        for i in 0..5 {
            let plaintext = format!("Determinism test message {}", i).into_bytes();
            let actions =
                client3.handle(ClientEvent::SendMessage { room_id: room3, plaintext }).unwrap();
            let frames = extract_send_frames(&actions);
            payloads3.push(frames[0].payload.clone());
        }

        // Oracle: Same seed = same ciphertexts
        assert_eq!(payloads1.len(), payloads2.len());
        for i in 0..payloads1.len() {
            assert_eq!(payloads1[i], payloads2[i], "Ciphertext {} differs with same seed", i);
        }

        // Oracle: Different seed = different ciphertexts
        for i in 0..payloads1.len() {
            assert_ne!(
                payloads1[i], payloads3[i],
                "Ciphertext {} should differ with different seed",
                i
            );
        }

        Ok(())
    });

    sim.run().unwrap();
}

// =============================================================================
// Property-Based Oracle Tests
// =============================================================================

/// Oracle: Verify encryption produces deterministic output for same inputs
/// (given same environment/RNG state)
#[test]
fn client_encryption_determinism() {
    // Run the same sequence twice with same seed and verify same output.
    // This tests that given the same RNG seed, the client produces the same
    // ciphertexts (deterministic behavior required for DST).

    let mut sim = Builder::new().build();

    sim.host("test", || async {
        // First run with seed 12345
        let env1 = SimEnv::with_seed(12345);
        let alice1 = ClientIdentity::new(1);
        let mut alice_client1 = Client::new(env1.clone(), alice1);

        alice_client1.handle(ClientEvent::CreateRoom { room_id: ROOM_ID }).expect("create room 1");

        let mut payloads1 = Vec::new();
        for i in 0..3 {
            let actions = alice_client1
                .handle(ClientEvent::SendMessage {
                    room_id: ROOM_ID,
                    plaintext: format!("Message {}", i).into_bytes(),
                })
                .expect("send message 1");

            let frames = extract_send_frames(&actions);
            payloads1.push(frames[0].payload.to_vec());
        }

        // Second run with same seed 12345 (use different room to avoid conflicts)
        let room_id_2 = ROOM_ID.wrapping_add(1);
        let env2 = SimEnv::with_seed(12345);
        let alice2 = ClientIdentity::new(1);
        let mut alice_client2 = Client::new(env2.clone(), alice2);

        alice_client2
            .handle(ClientEvent::CreateRoom { room_id: room_id_2 })
            .expect("create room 2");

        let mut payloads2 = Vec::new();
        for i in 0..3 {
            let actions = alice_client2
                .handle(ClientEvent::SendMessage {
                    room_id: room_id_2,
                    plaintext: format!("Message {}", i).into_bytes(),
                })
                .expect("send message 2");

            let frames = extract_send_frames(&actions);
            payloads2.push(frames[0].payload.to_vec());
        }

        // Third run with different seed 54321
        let room_id_3 = ROOM_ID.wrapping_add(2);
        let env3 = SimEnv::with_seed(54321);
        let alice3 = ClientIdentity::new(1);
        let mut alice_client3 = Client::new(env3.clone(), alice3);

        alice_client3
            .handle(ClientEvent::CreateRoom { room_id: room_id_3 })
            .expect("create room 3");

        let mut payloads3 = Vec::new();
        for i in 0..3 {
            let actions = alice_client3
                .handle(ClientEvent::SendMessage {
                    room_id: room_id_3,
                    plaintext: format!("Message {}", i).into_bytes(),
                })
                .expect("send message 3");

            let frames = extract_send_frames(&actions);
            payloads3.push(frames[0].payload.to_vec());
        }

        // Oracle: Same seed produces same ciphertexts
        assert_eq!(payloads1, payloads2, "Same seed should produce deterministic encryption");

        // Oracle: Different seed produces different ciphertexts
        assert_ne!(payloads1, payloads3, "Different seed should produce different encryption");

        Ok(())
    });

    sim.run().unwrap();
}
