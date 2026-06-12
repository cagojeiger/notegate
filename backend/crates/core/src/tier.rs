//! User tier quotas.
//!
//! System hard limits in [`crate::limits`] are the absolute ceiling. A user tier
//! may only lower those caps.

use crate::limits::{self, Limits};
use crate::{Error, Result};

/// Product tier attached to a user account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserTier {
    /// Default signup tier.
    Tier0,
    /// Internal/admin tier equal to the system hard maxima.
    SystemMax,
}

impl UserTier {
    pub const DEFAULT: Self = Self::Tier0;

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tier0 => "tier0",
            Self::SystemMax => "system_max",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "tier0" => Some(Self::Tier0),
            "system_max" => Some(Self::SystemMax),
            _ => None,
        }
    }

    pub fn parse_db(value: &str) -> Result<Self> {
        Self::parse(value).ok_or_else(|| Error::internal(format!("unknown user tier: {value}")))
    }

    pub fn quota(self) -> TierQuota {
        match self {
            Self::Tier0 => TierQuota {
                spaces_per_user: 1,
                agents_per_user: 3,
                connections_per_space: 5,
                connected_spaces_per_agent: 5,
                file_tree: Limits {
                    space_max_nodes: 2_000,
                    space_max_content_bytes: 134_217_728,
                    folder_max_children: 200,
                },
            },
            Self::SystemMax => TierQuota {
                spaces_per_user: limits::OWNED_SPACES_MAX,
                agents_per_user: limits::AGENTS_PER_CREATOR_MAX,
                connections_per_space: limits::CONNECTIONS_PER_SPACE_MAX,
                connected_spaces_per_agent: limits::CONNECTED_SPACES_PER_AGENT_MAX,
                file_tree: Limits::default(),
            },
        }
    }
}

/// Quotas that are tier-dependent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TierQuota {
    pub spaces_per_user: usize,
    pub agents_per_user: usize,
    pub connections_per_space: usize,
    pub connected_spaces_per_agent: usize,
    pub file_tree: Limits,
}

/// Apply runtime/dev caps on top of the user's tier quota.
pub fn effective_file_tree_limits(tier: UserTier, runtime_caps: Limits) -> Limits {
    let tier_caps = tier.quota().file_tree;
    Limits {
        space_max_nodes: tier_caps.space_max_nodes.min(runtime_caps.space_max_nodes),
        space_max_content_bytes: tier_caps
            .space_max_content_bytes
            .min(runtime_caps.space_max_content_bytes),
        folder_max_children: tier_caps
            .folder_max_children
            .min(runtime_caps.folder_max_children),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_max_matches_hard_limits() {
        let quota = UserTier::SystemMax.quota();
        assert_eq!(quota.spaces_per_user, limits::OWNED_SPACES_MAX);
        assert_eq!(quota.agents_per_user, limits::AGENTS_PER_CREATOR_MAX);
        assert_eq!(
            quota.connections_per_space,
            limits::CONNECTIONS_PER_SPACE_MAX
        );
        assert_eq!(
            quota.connected_spaces_per_agent,
            limits::CONNECTED_SPACES_PER_AGENT_MAX
        );
        assert_eq!(quota.file_tree, Limits::default());
    }

    #[test]
    fn tiers_do_not_exceed_hard_limits() {
        for tier in [UserTier::Tier0, UserTier::SystemMax] {
            let quota = tier.quota();
            assert!(quota.spaces_per_user <= limits::OWNED_SPACES_MAX);
            assert!(quota.agents_per_user <= limits::AGENTS_PER_CREATOR_MAX);
            assert!(quota.connections_per_space <= limits::CONNECTIONS_PER_SPACE_MAX);
            assert!(quota.connected_spaces_per_agent <= limits::CONNECTED_SPACES_PER_AGENT_MAX);
            assert!(quota.file_tree.space_max_nodes <= limits::SPACE_MAX_NODES);
            assert!(quota.file_tree.space_max_content_bytes <= limits::SPACE_MAX_CONTENT_BYTES);
            assert!(quota.file_tree.folder_max_children <= limits::FOLDER_MAX_CHILDREN);
        }
    }

    #[test]
    fn runtime_caps_can_only_lower_tier_caps() {
        let runtime = Limits {
            space_max_nodes: 10,
            space_max_content_bytes: 1024,
            folder_max_children: 3,
        };
        assert_eq!(
            effective_file_tree_limits(UserTier::SystemMax, runtime),
            runtime
        );
    }
}
