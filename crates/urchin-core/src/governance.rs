use serde::{Deserialize, Serialize};

/// How long events are retained before expiry or compaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionPolicy {
    pub hot_days: u32,
    pub warm_days: u32,
    /// Cold tier is kept indefinitely when true.
    pub cold_indefinite: bool,
}

/// Access tier assigned to a brain namespace or individual event.
/// Governs retention windows and, eventually, access-control decisions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AccessTier {
    Public,
    Private,
    Restricted,
    Confidential,
}

/// Immutability contract for a journal.
/// Events are append-only by invariant. Any deletion must produce an audit record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImmutabilityContract {
    pub audit_required_on_delete: bool,
    pub append_only: bool,
}

impl Default for ImmutabilityContract {
    fn default() -> Self {
        Self {
            audit_required_on_delete: true,
            append_only: true,
        }
    }
}

/// Per-journal governance configuration.
/// Unset fields inherit daemon-level defaults once defaults are implemented.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GovernanceConfig {
    pub retention: Option<RetentionPolicy>,
    pub access_tier: Option<AccessTier>,
    pub immutability: Option<ImmutabilityContract>,
}
