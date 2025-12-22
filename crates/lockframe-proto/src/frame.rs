//! Frame type combining header and payload.
//!
//! A `Frame` is the transport-layer packet consisting of:
//! - 128-byte raw binary header (Big Endian) for O(1) routing
//! - Variable-length raw bytes (already encoded)
//!
//! This is a pure data holder (header + bytes). For high-level logic,
//! see `Payload::into_frame()` and `Payload::from_frame()`.

use bytes::{BufMut, Bytes};

use crate::{
    FrameHeader,
    errors::{ProtocolError, Result},
};

/// Complete protocol frame (transport layer)
///
/// Layout on the wire:
/// `[FrameHeader: 128 bytes, raw binary] + [payload: variable bytes]`
///
/// This type holds raw bytes, NOT the `Payload` enum. This allows the
/// server to route frames without deserializing the payload.
///
/// # Invariants
///
/// - **Size Consistency**: `payload.len()` MUST match `header.payload_size()`.
///   This invariant is enforced by [`Frame::new`] and verified by
///   [`Frame::decode`].
///
/// - **Size Limit**: `payload.len()` MUST NOT exceed
///   [`FrameHeader::MAX_PAYLOAD_SIZE`] (16 MB). Violations are rejected during
///   construction and encoding.
///
/// # Security
///
/// This struct provides **structural validity** only. It guarantees:
/// - Valid header format (magic number, version, size limits)
/// - Payload size matches header claim
///
/// It does **NOT** guarantee:
/// - Authentication (signature verification must be done separately)
/// - Decryption (payload may be ciphertext)
/// - CBOR validity (payload deserialization happens later)
///
/// For authenticated content, the signature in `header.signature()` must be
/// verified against the MLS epoch before trusting the payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Frame header (128 bytes)
    pub header: FrameHeader,

    /// Raw payload bytes (already CBOR-encoded)
    pub payload: Bytes,
}

impl Frame {
    /// Create a new frame with automatic payload_size calculation
    ///
    /// The header's `payload_size` field is automatically set to match
    /// the actual payload length, ensuring consistency.
    ///
    /// # Panics
    ///
    /// Panics if `payload.len() > u32::MAX`. In practice, this cannot happen
    /// because `Bytes` is bounded by `isize::MAX` which is smaller than
    /// `u32::MAX` on all supported platforms.
    ///
    /// # Security
    ///
    /// - **Size Enforcement**: The payload size is set automatically, making it
    ///   impossible to create a Frame with mismatched header and payload sizes.
    ///   This prevents desynchronization attacks where the header claims a
    ///   different size than the payload.
    ///
    /// - **No Validation**: This constructor does NOT validate that payload
    ///   size is under [`FrameHeader::MAX_PAYLOAD_SIZE`]. Oversized frames will
    ///   be rejected later during [`Frame::encode`]. This design allows
    ///   constructing frames for testing without artificial size restrictions.
    #[must_use]
    pub fn new(mut header: FrameHeader, payload: impl Into<Bytes>) -> Self {
        let payload = payload.into();
        header.payload_size = (payload.len() as u32).to_be_bytes();

        debug_assert_eq!(header.payload_size(), payload.len() as u32);

        Self { header, payload }
    }

    /// Encode frame into buffer (simple copy, no magic)
    ///
    /// Writes: `[header (128 bytes)] + [payload (variable)]`
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError::PayloadTooLarge`] if payload exceeds
    /// [`FrameHeader::MAX_PAYLOAD_SIZE`] (16 MB).
    ///
    /// # Security
    ///
    /// - **Size Limit Enforcement**: This is the enforcement point for the 16
    ///   MB payload limit. Frames exceeding this size are rejected to prevent
    ///   memory exhaustion DoS attacks.
    ///
    /// - **No Serialization**: This function performs simple memory copies with
    ///   no parsing or transformation. There are no opportunities for injection
    ///   or corruption.
    pub fn encode(&self, dst: &mut impl BufMut) -> Result<()> {
        debug_assert_eq!(self.payload.len(), self.header.payload_size() as usize);

        if self.payload.len() > FrameHeader::MAX_PAYLOAD_SIZE as usize {
            return Err(ProtocolError::PayloadTooLarge {
                size: self.payload.len(),
                max: FrameHeader::MAX_PAYLOAD_SIZE as usize,
            });
        }

        dst.put_slice(&self.header.to_bytes());
        dst.put_slice(&self.payload);

        Ok(())
    }

    /// Decode frame from wire format
    ///
    /// Returns a Frame with raw bytes (does NOT deserialize payload).
    /// Use `Payload::from_frame()` if you need the high-level enum.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Header parsing fails (invalid magic, version, or size limits)
    /// - Payload is truncated (fewer bytes than header claims)
    ///
    /// # Security
    ///
    /// - **Fail Fast**: All validation happens before allocating memory for the
    ///   payload. Malformed headers are rejected without copying data.
    ///
    /// - **Exact Size**: We only read exactly `payload_size` bytes from the
    ///   buffer. Trailing data is ignored, preventing buffer over-read.
    ///
    /// - **No Deserialization**: This function does NOT parse CBOR. It only
    ///   validates structural framing. Payload deserialization happens later
    ///   with explicit error handling.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let header = FrameHeader::from_bytes(bytes)?;

        let payload_size = header.payload_size() as usize;
        let total_size = FrameHeader::SIZE.checked_add(payload_size).ok_or_else(|| {
            ProtocolError::PayloadTooLarge {
                size: payload_size,
                max: FrameHeader::MAX_PAYLOAD_SIZE as usize,
            }
        })?;

        debug_assert!(total_size >= FrameHeader::SIZE);

        if bytes.len() < total_size {
            #[cfg_attr(not(fuzzing), allow(unexpected_cfgs))]
            #[cfg(fuzzing)]
            {
                let _ = payload_size; // Proves we hit this branch
            }

            return Err(ProtocolError::FrameTruncated {
                expected: payload_size,
                actual: bytes.len().saturating_sub(FrameHeader::SIZE),
            });
        }

        #[cfg_attr(not(fuzzing), allow(unexpected_cfgs))]
        #[cfg(fuzzing)]
        {
            let _ = total_size; // Proves we hit success path
        }

        let payload = Bytes::copy_from_slice(&bytes[FrameHeader::SIZE..total_size]);

        debug_assert_eq!(payload.len(), payload_size);

        Ok(Self { header: *header, payload })
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::Opcode;

    impl Arbitrary for Frame {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
            (any::<FrameHeader>(), any::<Vec<u8>>())
                .prop_map(|(header, payload_bytes)| Frame::new(header, payload_bytes))
                .boxed()
        }
    }

    proptest! {
        #[test]
        fn frame_round_trip(frame in any::<Frame>()) {
            let mut wire = Vec::new();
            frame.encode(&mut wire).expect("should encode");

            let parsed = Frame::decode(&wire).expect("should decode");
            prop_assert_eq!(frame.payload, parsed.payload);
        }
    }

    #[test]
    fn frame_with_payload() {
        // Create valid header
        let mut bytes = [0u8; 128];
        bytes[0..4].copy_from_slice(&FrameHeader::MAGIC.to_be_bytes());
        bytes[4] = FrameHeader::VERSION;
        let mut header = *FrameHeader::from_bytes(&bytes).unwrap();
        header.opcode = Opcode::Ping.to_u16().to_be_bytes();

        // Create frame (payload_size set automatically)
        let payload_bytes = vec![1, 2, 3, 4];
        let frame = Frame::new(header, payload_bytes.clone());

        // Verify payload_size was set correctly
        assert_eq!(frame.header.payload_size(), payload_bytes.len() as u32);

        // Encode and decode
        let mut wire = Vec::new();
        frame.encode(&mut wire).expect("should encode");

        let parsed = Frame::decode(&wire).expect("should decode");
        assert_eq!(frame.payload, parsed.payload);
    }

    #[test]
    fn reject_truncated_frame() {
        // Create header claiming 100 bytes of payload
        let mut bytes = [0u8; 128];
        bytes[0..4].copy_from_slice(&FrameHeader::MAGIC.to_be_bytes());
        bytes[4] = FrameHeader::VERSION;

        let mut header = *FrameHeader::from_bytes(&bytes).unwrap();
        header.payload_size = 100u32.to_be_bytes();

        let header_bytes = header.to_bytes();

        // Only provide header, no payload
        let result = Frame::decode(&header_bytes);
        assert!(matches!(result, Err(ProtocolError::FrameTruncated { .. })));
    }
}
