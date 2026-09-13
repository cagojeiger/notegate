use notegate_model::{Channel, Node};

use crate::{ServiceError, ServiceResult};

use super::FilesService;

impl FilesService {
    pub(super) async fn require_node_access(&self, node: &Node) -> ServiceResult<()> {
        if self.channel == Channel::Browser {
            return Ok(());
        }
        require_access(self.channel, node.external_access_enabled)?;
        let allowed = self
            .store
            .externally_accessible_node_ids(node.space_id, &[node.id], false)
            .await?;
        require_access(self.channel, allowed.contains(&node.id))
    }
}

fn require_access(channel: Channel, enabled: bool) -> ServiceResult<()> {
    if channel != Channel::Browser && !enabled {
        // Do not disclose whether a browser-only node exists to external callers.
        return Err(ServiceError::NotFound("node not found".to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_access_is_independent_of_external_policy() {
        for enabled in [false, true] {
            assert!(require_access(Channel::Browser, enabled).is_ok());
        }
    }

    #[test]
    fn mcp_and_api_use_the_same_node_policy() {
        for channel in [Channel::Mcp, Channel::Api] {
            assert!(require_access(channel, true).is_ok());
            assert!(matches!(
                require_access(channel, false),
                Err(ServiceError::NotFound(_))
            ));
        }
    }
}
