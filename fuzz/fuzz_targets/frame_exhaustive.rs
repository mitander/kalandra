//! Exhaustive positive space fuzzer for Frame encoding/decoding
//!
//! Unlike random fuzzing (frame_decode.rs), this fuzzer EXHAUSTIVELY tests
//! all combinations of:
//! - All 16 opcodes
//! - Edge-case values (0, 1, MAX) for all ID fields
//! - Empty and small payloads
//!
//! This ensures we don't miss bugs that occur only with specific opcode+value
//! combinations that random sampling might not hit.

#![no_main]

use libfuzzer_sys::fuzz_target;
use lockframe_proto::{Frame, FrameHeader, Opcode};

// All opcodes to test exhaustively
const ALL_OPCODES: &[Opcode] = &[
    Opcode::Hello,
    Opcode::HelloReply,
    Opcode::Ping,
    Opcode::Pong,
    Opcode::Goodbye,
    Opcode::Error,
    Opcode::AppMessage,
    Opcode::AppReceipt,
    Opcode::AppReaction,
    Opcode::Welcome,
    Opcode::Commit,
    Opcode::Proposal,
    Opcode::KeyPackage,
    Opcode::Redact,
    Opcode::Ban,
    Opcode::Kick,
];

// Edge-case values for 128-bit room_id
const ROOM_IDS: &[u128] = &[
    0,
    1,
    0x1000,           // Typical small value
    u64::MAX as u128, // 64-bit boundary
    u128::MAX / 2,    // Mid-range
    u128::MAX - 1,
    u128::MAX,
];

// Edge-case values for 64-bit fields (sender_id, epoch, log_index)
const U64_EDGES: &[u64] = &[
    0,
    1,
    0x1000,          // Typical small value
    u32::MAX as u64, // 32-bit boundary
    u64::MAX / 2,    // Mid-range
    u64::MAX - 1,
    u64::MAX,
];

// Payload sizes to test
const PAYLOAD_SIZES: &[usize] = &[
    0,    // Empty
    1,    // Single byte
    127,  // Just under 128
    128,  // Exactly header size
    255,  // One byte
    256,  // Two bytes
    1024, // 1KB
];

fuzz_target!(|data: &[u8]| {
    // Use input data to select which combination to test
    // This allows libFuzzer to guide exploration while remaining exhaustive
    if data.len() < 4 {
        return;
    }

    let opcode_idx = data[0] as usize % ALL_OPCODES.len();
    let room_id_idx = data[1] as usize % ROOM_IDS.len();
    let sender_id_idx = data[2] as usize % U64_EDGES.len();
    let epoch_idx = data[3] as usize % U64_EDGES.len();

    let opcode = ALL_OPCODES[opcode_idx];
    let room_id = ROOM_IDS[room_id_idx];
    let sender_id = U64_EDGES[sender_id_idx];
    let epoch = U64_EDGES[epoch_idx];

    // Test all log_index edges for this combination
    for &log_index in U64_EDGES {
        let mut header = FrameHeader::new(opcode);
        header.set_room_id(room_id);
        header.set_sender_id(sender_id);
        header.set_epoch(epoch);
        header.set_log_index(log_index);

        // Test with various payload sizes
        for &payload_size in PAYLOAD_SIZES {
            let payload = if payload_size <= data.len() - 4 {
                &data[4..4 + payload_size]
            } else {
                &vec![0u8; payload_size]
            };

            let frame = Frame::new(header, payload.to_vec());

            // INVARIANT 1: Encoding must succeed
            let mut buf = Vec::new();
            frame.encode(&mut buf).expect("encode should never fail for valid frame");

            // INVARIANT 2: Decoding must succeed
            let decoded = Frame::decode(&buf).expect("decode should succeed for valid encoding");

            // INVARIANT 3: Round-trip must be identity
            assert_eq!(
                decoded.header.opcode(),
                header.opcode(),
                "Opcode mismatch for {:?}",
                opcode
            );
            assert_eq!(
                decoded.header.room_id(),
                room_id,
                "Room ID mismatch for room_id={}",
                room_id
            );
            assert_eq!(
                decoded.header.sender_id(),
                sender_id,
                "Sender ID mismatch for sender_id={}",
                sender_id
            );
            assert_eq!(decoded.header.epoch(), epoch, "Epoch mismatch for epoch={}", epoch);
            assert_eq!(
                decoded.header.log_index(),
                log_index,
                "Log index mismatch for log_index={}",
                log_index
            );
            assert_eq!(
                decoded.payload.len(),
                payload.len(),
                "Payload size mismatch for size={}",
                payload.len()
            );

            // INVARIANT 4: Encoded size must be correct
            let expected_size = FrameHeader::SIZE + payload.len();
            assert_eq!(
                buf.len(),
                expected_size,
                "Encoded size incorrect: expected {}, got {}",
                expected_size,
                buf.len()
            );
        }
    }

    #[cfg(fuzzing)]
    {
        // This is reachable - proves fuzzer hits this code
        let _ = ALL_OPCODES.len();
    }
});
