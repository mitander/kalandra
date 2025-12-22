//! Moderation operation payload types.
//!
//! These payloads allow moderators to manage content and users.
//! All moderation actions are logged and auditable.

use serde::{Deserialize, Serialize};

/// Redact message content
///
/// Removes message content via cryptographic erasure (deleting the payload
/// key).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Redact {
    /// Log index of the message to redact
    pub message_log_index: u64,

    /// Reason for redaction
    pub reason: String,

    /// Moderator ID
    pub moderator_id: u64,
}

/// Ban user from room
///
/// Removes user via MLS External Commit, preventing future message decryption.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ban {
    /// User ID to ban
    pub user_id: u64,

    /// Reason for ban
    pub reason: String,

    /// Ban duration in seconds (None = permanent)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<u64>,

    /// Moderator ID
    pub moderator_id: u64,
}

/// Kick user from room
///
/// Temporary removal without ban. User can rejoin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kick {
    /// User ID to kick
    pub user_id: u64,

    /// Reason for kick
    pub reason: String,

    /// Moderator ID
    pub moderator_id: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_serde() {
        let redact = Redact { message_log_index: 100, reason: "Spam".to_string(), moderator_id: 1 };

        let cbor = ciborium::ser::into_writer(&redact, Vec::new());
        assert!(cbor.is_ok());
    }

    #[test]
    fn ban_serde() {
        let ban = Ban {
            user_id: 42,
            reason: "Violation".to_string(),
            duration_secs: Some(86400),
            moderator_id: 1,
        };

        let cbor = ciborium::ser::into_writer(&ban, Vec::new());
        assert!(cbor.is_ok());
    }
}
