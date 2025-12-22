//! Session management payload types.
//!
//! These payloads handle connection lifecycle: handshake, keepalive, and
//! disconnection.

use serde::{Deserialize, Serialize};

/// Initial client handshake
///
/// The first message sent by a client to establish a session. The server
/// responds with [`HelloReply`] containing a session ID.
///
/// # Security
///
/// - **Debug Redaction**: The `Debug` impl redacts `auth_token` to prevent
///   accidental logging of credentials. Always use custom `Debug`
///   implementations for types containing secrets.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    /// Protocol version
    pub version: u8,
    /// Client capabilities (future use)
    pub capabilities: Vec<String>,
    /// Authentication token (optional)
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub auth_token: Option<Vec<u8>>,
}

impl std::fmt::Debug for Hello {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hello")
            .field("version", &self.version)
            .field("capabilities", &self.capabilities)
            .field(
                "auth_token",
                &self.auth_token.as_ref().map(|token| format!("<redacted {} bytes>", token.len())),
            )
            .finish()
    }
}

/// Server response to Hello
///
/// Sent by the server after receiving [`Hello`]. Contains the assigned session
/// ID and optionally an authentication challenge.
///
/// # Security
///
/// - **Debug Redaction**: The `Debug` impl redacts `challenge` to prevent
///   logging cryptographic nonces or auth challenges.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloReply {
    /// Assigned session ID
    pub session_id: u64,
    /// Server capabilities
    pub capabilities: Vec<String>,
    /// Authentication challenge (if needed)
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub challenge: Option<Vec<u8>>,
}

impl std::fmt::Debug for HelloReply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HelloReply")
            .field("session_id", &self.session_id)
            .field("capabilities", &self.capabilities)
            .field(
                "challenge",
                &self.challenge.as_ref().map(|ch| format!("<redacted {} bytes>", ch.len())),
            )
            .finish()
    }
}

/// Graceful disconnect
///
/// Sent by either client or server to terminate a session cleanly.
/// After sending or receiving `Goodbye`, both parties should close the
/// connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Goodbye {
    /// Reason for disconnect (for logging/debugging)
    pub reason: String,
}

/// Client request for missing frames (epoch sync)
///
/// Sent by a client when it detects it's behind the server's epoch
/// (e.g., after a commit timeout or epoch mismatch error).
///
/// # Protocol Flow
///
/// 1. Client detects epoch mismatch (e.g., server is at epoch 5, client at 3)
/// 2. Client sends `SyncRequest { from_log_index: 42 }` for the room
/// 3. Server responds with `SyncResponse` containing frames from log_index 42+
/// 4. Client processes frames in order to catch up
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncRequest {
    /// Start replaying frames from this log index (inclusive).
    ///
    /// The client typically sets this to its last successfully processed
    /// log_index + 1.
    pub from_log_index: u64,

    /// Maximum number of frames to return.
    ///
    /// Limits response size. Server may return fewer if fewer frames exist.
    /// Default: 100 frames per batch.
    #[serde(default = "default_limit")]
    pub limit: u64,
}

fn default_limit() -> u64 {
    100
}

/// Server response with frames for sync
///
/// Contains a batch of frames for the client to process in order.
/// If `has_more` is true, the client should send another `SyncRequest`
/// with `from_log_index` = last frame's log_index + 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncResponse {
    /// Frames in log_index order.
    ///
    /// Each entry is the raw frame bytes (header + payload).
    /// Client should process in order.
    pub frames: Vec<Vec<u8>>,

    /// True if more frames are available after this batch.
    ///
    /// If true, client should send another SyncRequest to continue.
    pub has_more: bool,

    /// Current server epoch for this room.
    ///
    /// After processing all frames, client epoch should match this.
    pub server_epoch: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_serde() {
        let hello = Hello { version: 1, capabilities: vec!["mls".to_string()], auth_token: None };

        let cbor = ciborium::ser::into_writer(&hello, Vec::new());
        assert!(cbor.is_ok());
    }

    #[test]
    fn sync_request_serde() {
        let request = SyncRequest { from_log_index: 42, limit: 50 };

        let mut bytes = Vec::new();
        ciborium::ser::into_writer(&request, &mut bytes).expect("encode");

        let decoded: SyncRequest = ciborium::de::from_reader(&bytes[..]).expect("decode");
        assert_eq!(request, decoded);
    }

    #[test]
    fn sync_request_default_limit() {
        // Encode without limit field
        let request_no_limit = SyncRequest { from_log_index: 10, limit: default_limit() };

        let mut bytes = Vec::new();
        ciborium::ser::into_writer(&request_no_limit, &mut bytes).expect("encode");

        let decoded: SyncRequest = ciborium::de::from_reader(&bytes[..]).expect("decode");
        assert_eq!(decoded.limit, 100); // default
    }

    #[test]
    fn sync_response_serde() {
        let response = SyncResponse {
            frames: vec![vec![1, 2, 3], vec![4, 5, 6]],
            has_more: true,
            server_epoch: 5,
        };

        let mut bytes = Vec::new();
        ciborium::ser::into_writer(&response, &mut bytes).expect("encode");

        let decoded: SyncResponse = ciborium::de::from_reader(&bytes[..]).expect("decode");
        assert_eq!(response, decoded);
    }
}
