//! # Lockframe Protocol: Wire Format
//!
//! This crate implements the binary framing layer for the Lockframe secure
//! messaging protocol.
//!
//! ## Protocol Design
//!
//! The protocol uses a hybrid encoding strategy optimized for high-throughput
//! routing:
//! - **FrameHeader**: 128 bytes of raw binary (Big Endian) for zero-copy
//!   routing
//! - **Payload**: Variable-length CBOR-encoded structured data
//!
//! ## Implementation Notes
//!
//! This implementation makes specific choices for memory safety and
//! performance:
//!
//! - **Zero-Copy Parsing**: We use [`zerocopy`](https://docs.rs/zerocopy) to
//!   cast network bytes directly to [`FrameHeader`] structures, avoiding
//!   serialization overhead on the hot path. This enables O(1) routing
//!   decisions at 15K+ frames/sec.
//!
//! - **Cache-Line Alignment**: The 128-byte header fits exactly into two
//!   64-byte CPU cache lines, with routing data in the first line and
//!   authentication in the second. This allows the sequencer to make routing
//!   decisions touching only 64 bytes of memory.
//!
//! - **CBOR for Payloads**: While the header is raw binary for performance,
//!   payloads use CBOR to maintain forward compatibility and type safety. The
//!   sequencer never deserializes payloads, only clients do.
//!
//! ## Security Properties
//!
//! - **No Unsafe Deserialization**: All parsing uses `zerocopy` with
//!   compile-time layout verification. Malformed frames are rejected before any
//!   data is copied.
//!
//! - **Size Limits**: The protocol enforces a 16 MB maximum payload size to
//!   prevent DoS attacks via memory exhaustion.
//!
//! - **Explicit Validation**: All constructors and parsing functions validate
//!   invariants and return `Result` types. There are no "unchecked" fast paths
//!   that skip validation.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod errors;
pub mod flags;
pub mod frame;
pub mod header;
pub mod opcodes;
pub mod payloads;

pub use errors::{ProtocolError, Result};
pub use flags::FrameFlags;
pub use frame::Frame;
pub use header::FrameHeader;
pub use opcodes::Opcode;
pub use payloads::Payload;
