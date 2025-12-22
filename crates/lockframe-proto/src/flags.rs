//! Frame flags for the Lockframe protocol.
//!
//! Flags are used to indicate optional frame properties like compression,
//! fragmentation, priority, etc.

use bitflags::bitflags;
use serde::{Deserialize, Serialize};

bitflags! {
    /// Frame feature flags (8 bits)
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(transparent)]
    pub struct FrameFlags: u8 {
        /// Payload is zstd compressed
        const COMPRESSED = 0b0000_0001;

        /// Part of a fragmented message
        const FRAGMENTED = 0b0000_0010;

        /// High priority delivery
        const PRIORITY = 0b0000_0100;

        /// From federated source (not local hub)
        const FEDERATED = 0b0000_1000;

        /// External sender (server-initiated)
        const EXTERNAL = 0b0001_0000;

        /// Don't persist to disk (ephemeral)
        const EPHEMERAL = 0b0010_0000;

        /// Can be redacted by moderators
        const REDACTABLE = 0b0100_0000;

        /// Reserved for future use
        const RESERVED = 0b1000_0000;
    }
}

impl FrameFlags {
    /// Create flags from raw byte value
    ///
    /// This function is **infallible** because `bitflags` represents flags as a
    /// simple `u8` wrapper. All 256 possible byte values are valid -
    /// unknown bits are preserved but ignored during flag checks.
    ///
    /// # Security
    ///
    /// - **No Validation Required**: Unlike enums, flag parsing cannot fail. An
    ///   attacker can set reserved bits, but this has no effect on behavior
    ///   since reserved bits are never checked.
    ///
    /// - **Forward Compatibility**: Future protocol versions can define new
    ///   flags in currently-reserved bits. Old clients will preserve but ignore
    ///   them.
    #[must_use]
    pub const fn from_byte(byte: u8) -> Self {
        Self::from_bits_retain(byte)
    }

    /// Convert to raw byte value
    #[must_use]
    pub const fn to_byte(self) -> u8 {
        self.bits()
    }
}

impl Default for FrameFlags {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_basic() {
        let flags = FrameFlags::COMPRESSED | FrameFlags::PRIORITY;
        assert!(flags.contains(FrameFlags::COMPRESSED));
        assert!(flags.contains(FrameFlags::PRIORITY));
        assert!(!flags.contains(FrameFlags::FEDERATED));
    }

    #[test]
    fn flags_round_trip() {
        let flags = FrameFlags::EXTERNAL | FrameFlags::REDACTABLE;
        let byte = flags.to_byte();
        let parsed = FrameFlags::from_byte(byte);
        assert_eq!(flags, parsed);
    }

    #[test]
    fn flags_empty() {
        let flags = FrameFlags::empty();
        assert_eq!(flags.to_byte(), 0);
    }

    #[test]
    fn flags_all() {
        let flags = FrameFlags::all();
        assert_eq!(flags.to_byte(), 0xFF);
    }
}
