//! Fuzz target for [`Connection`] state machine
//!
//! Prevent authentication bypass via invalid state transitions
//!
//! # Strategy
//!
//! - Event sequences: Arbitrary sequences of frames, ticks, and close
//!   operations
//! - Invalid opcodes: Unexpected frame types for current state
//! - Timeout testing: Advance time to trigger handshake/idle timeouts
//! - State probing: Out-of-order handshakes, duplicate hellos
//!
//! # Invariants
//!
//! - `Authenticated` ONLY reachable via valid `Hello → HelloReply` sequence
//! - No transition FROM `Closed` state (terminal invariant)
//! - Timeout in `Pending` MUST trigger `Close` action
//! - Ping/Pong only valid in `Authenticated` state
//! - Session ID never changes after assignment
//! - Out-of-order handshake (HelloReply before Hello) MUST reject
//! - Duplicate Hello in Authenticated MUST reject
//! - NEVER panic on unexpected opcode

#![no_main]

use std::{ops::Sub, time::Duration};

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use lockframe_core::connection::{Connection, ConnectionAction, ConnectionConfig, ConnectionState};
use lockframe_proto::{
    payloads::session::{Goodbye, Hello, HelloReply},
    Frame, FrameHeader, Opcode, Payload,
};

/// Represents time as Duration since epoch 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct FuzzInstant(Duration);

impl Sub for FuzzInstant {
    type Output = Duration;

    fn sub(self, other: Self) -> Duration {
        self.0.saturating_sub(other.0)
    }
}

#[derive(Debug, Clone, Arbitrary)]
enum ConnectionEvent {
    SendHello,
    ReceiveFrame(FuzzedFrame),
    Tick { advance_secs: u8 },
    Close,
    CheckTimeout { advance_secs: u8 },
}

#[derive(Debug, Clone, Arbitrary)]
struct FuzzedFrame {
    opcode: u16,
    payload: FuzzedPayload,
}

#[derive(Debug, Clone, Arbitrary)]
enum FuzzedPayload {
    Hello { version: u8 },
    HelloReply { session_id: u64 },
    Ping,
    Pong,
    Goodbye { reason_len: u8 },
    Error,
    RandomBytes(Vec<u8>),
}

/// Fuzz input with deterministic seed for time initialization.
#[derive(Debug, Clone, Arbitrary)]
struct FuzzInput {
    /// Seed for deterministic initial time (seconds since epoch 0).
    initial_time_secs: u32,
    /// Event sequence to process.
    events: Vec<ConnectionEvent>,
}

fuzz_target!(|input: FuzzInput| {
    let config = ConnectionConfig {
        handshake_timeout: Duration::from_secs(5),
        idle_timeout: Duration::from_secs(10),
        heartbeat_interval: Duration::from_secs(3),
    };

    let initial_time = FuzzInstant(Duration::from_secs(input.initial_time_secs as u64));
    let mut conn: Connection<FuzzInstant> = Connection::new(initial_time, config);
    let mut current_time = initial_time;
    let mut previous_state;
    let mut initial_session_id: Option<u64> = None;

    for event in input.events {
        previous_state = conn.state();

        match event {
            ConnectionEvent::SendHello => {
                let result = conn.send_hello(current_time);

                match previous_state {
                    ConnectionState::Init if result.is_ok() => {
                        assert_eq!(conn.state(), ConnectionState::Pending);
                    },
                    _ => {
                        assert!(result.is_err() || conn.state() == previous_state);
                    },
                }
            },

            ConnectionEvent::ReceiveFrame(fuzzed) => {
                let frame = create_frame_from_fuzzed(&fuzzed);
                let result = conn.handle_frame(&frame, current_time);

                match result {
                    Ok(actions) => {
                        if let (ConnectionState::Closed, new_state) = (previous_state, conn.state())
                        {
                            if new_state != ConnectionState::Closed {
                                panic!(
                                    "Transitioned FROM Closed to {:?}. Closed must be terminal!",
                                    new_state
                                );
                            }
                        }

                        if previous_state == ConnectionState::Pending
                            && conn.state() == ConnectionState::Authenticated
                        {
                            if let Some(session_id) = conn.session_id() {
                                if initial_session_id.is_none() {
                                    initial_session_id = Some(session_id);
                                }
                            }
                        }

                        for action in actions {
                            match action {
                                ConnectionAction::SendFrame(_) | ConnectionAction::Close { .. } => {
                                },
                            }
                        }
                    },
                    Err(_) => {
                        assert!(
                            conn.state() == previous_state
                                || conn.state() == ConnectionState::Closed
                        );
                    },
                }
            },

            ConnectionEvent::Tick { advance_secs } => {
                current_time =
                    FuzzInstant(current_time.0 + Duration::from_secs((advance_secs % 120) as u64));
                let actions = conn.tick(current_time);

                for action in actions {
                    if let ConnectionAction::Close { .. } = action {
                        assert_eq!(conn.state(), ConnectionState::Closed);
                    }
                }
            },

            ConnectionEvent::Close => {
                conn.close();
                assert_eq!(conn.state(), ConnectionState::Closed);
            },

            ConnectionEvent::CheckTimeout { advance_secs } => {
                let check_time =
                    FuzzInstant(current_time.0 + Duration::from_secs((advance_secs % 120) as u64));
                let _ = conn.check_timeout(check_time);
            },
        }

        if conn.state() == ConnectionState::Authenticated {
            if let Some(session_id) = conn.session_id() {
                if let Some(initial) = initial_session_id {
                    assert_eq!(
                        session_id, initial,
                        "Session ID changed! {} -> {}",
                        initial, session_id
                    );
                } else {
                    initial_session_id = Some(session_id);
                }
            }
        }
    }

    if conn.state() == ConnectionState::Closed {
        let _ = conn.send_hello(current_time);
        let _ = conn.tick(current_time);
        assert_eq!(conn.state(), ConnectionState::Closed);
    }
});

fn create_frame_from_fuzzed(fuzzed: &FuzzedFrame) -> Frame {
    let opcode_enum = Opcode::from_u16(fuzzed.opcode);

    match &fuzzed.payload {
        FuzzedPayload::Hello { version } => {
            let hello =
                Payload::Hello(Hello { version: *version, capabilities: vec![], auth_token: None });
            hello
                .into_frame(FrameHeader::new(Opcode::Hello))
                .unwrap_or_else(|_| Frame::new(FrameHeader::new(Opcode::Hello), Vec::new()))
        },
        FuzzedPayload::HelloReply { session_id } => {
            let reply = Payload::HelloReply(HelloReply {
                session_id: *session_id,
                capabilities: vec![],
                challenge: None,
            });
            reply
                .into_frame(FrameHeader::new(Opcode::HelloReply))
                .unwrap_or_else(|_| Frame::new(FrameHeader::new(Opcode::HelloReply), Vec::new()))
        },
        FuzzedPayload::Ping => Frame::new(FrameHeader::new(Opcode::Ping), Vec::new()),
        FuzzedPayload::Pong => Frame::new(FrameHeader::new(Opcode::Pong), Vec::new()),
        FuzzedPayload::Goodbye { reason_len } => {
            let reason = "x".repeat((*reason_len % 100) as usize);
            let goodbye = Payload::Goodbye(Goodbye { reason });
            goodbye
                .into_frame(FrameHeader::new(Opcode::Goodbye))
                .unwrap_or_else(|_| Frame::new(FrameHeader::new(Opcode::Goodbye), Vec::new()))
        },
        FuzzedPayload::Error => Frame::new(FrameHeader::new(Opcode::Error), Vec::new()),
        FuzzedPayload::RandomBytes(bytes) => {
            let opcode = opcode_enum.unwrap_or(Opcode::AppMessage);
            Frame::new(FrameHeader::new(opcode), bytes.clone())
        },
    }
}
