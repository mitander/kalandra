//! MLS operation payload types.
//!
//! These types wrap raw MLS protocol data. The actual MLS cryptographic
//! operations are handled by the `openmls` library in higher layers.

use serde::{Deserialize, Serialize};

/// Key package upload
///
/// Contains a serialized MLS KeyPackage for joining groups.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyPackageData {
    /// Serialized MLS KeyPackage (from openmls)
    pub key_package_bytes: Vec<u8>,
}

/// MLS proposal
///
/// Proposals are staged changes to the group (add member, remove member, etc.)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalData {
    /// Serialized MLS Proposal
    pub proposal_bytes: Vec<u8>,

    /// Proposal type hint (for routing/logging)
    pub proposal_type: ProposalType,
}

/// Type of MLS proposal
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[allow(clippy::upper_case_acronyms)]
pub enum ProposalType {
    /// Add a new member
    Add,
    /// Remove an existing member
    Remove,
    /// Update own key material
    Update,
    /// Pre-shared key
    PSK,
    /// Reinitialize the group with different parameters
    ReInit,
    /// External initialization proposal
    ExternalInit,
    /// Modify group context extensions
    GroupContextExtensions,
}

/// MLS commit
///
/// Commits apply one or more proposals and advance the epoch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitData {
    /// Serialized MLS Commit
    pub commit_bytes: Vec<u8>,

    /// New epoch number
    pub new_epoch: u64,

    /// Tree hash after commit
    pub tree_hash: [u8; 32],

    /// True if this is an external commit (from server or new joiner)
    pub is_external: bool,
}

/// MLS welcome message
///
/// Sent to new members joining the group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WelcomeData {
    /// Serialized MLS Welcome
    pub welcome_bytes: Vec<u8>,

    /// Epoch the new member will join at
    pub epoch: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_data_serde() {
        let commit = CommitData {
            commit_bytes: vec![1, 2, 3],
            new_epoch: 42,
            tree_hash: [0; 32],
            is_external: false,
        };

        let cbor = ciborium::ser::into_writer(&commit, Vec::new());
        assert!(cbor.is_ok());
    }
}
