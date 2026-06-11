//! Space agent connections: list / connect / disconnect.
//!
//! Users manage their own agents. A connection grants one owned agent `read` or
//! `write` permission inside one owned space.

use notegate_core::limits;
use notegate_db::ConnectionRepo;
pub use notegate_model::{
    AccountKind, ConnectAgent, ConnectionPage, ListConnections, SpaceAgentConnection,
};
use uuid::Uuid;

use crate::error::{ServiceError, ServiceResult};
use crate::pagination::{clamp_limit, paginate_by_id};

#[derive(Debug, Clone)]
pub struct ConnectionService {
    store: ConnectionRepo,
}

impl ConnectionService {
    pub fn new(store: ConnectionRepo) -> Self {
        Self { store }
    }

    pub async fn list_page(
        &self,
        caller_kind: AccountKind,
        caller_user_id: Uuid,
        space_id: Uuid,
        request: ListConnections,
    ) -> ServiceResult<ConnectionPage> {
        require_user_caller(caller_kind)?;
        let limit = clamp_limit(
            request.limit,
            limits::CONNECTIONS_DEFAULT_LIMIT,
            limits::CONNECTIONS_MAX_LIMIT,
        );
        let connections = self
            .store
            .list_connections(space_id, caller_user_id)
            .await?;
        let (items, has_more, next_cursor) = paginate_by_id(
            connections,
            |connection| connection.agent_id,
            limit,
            request.cursor.as_deref(),
        )?;
        Ok(ConnectionPage {
            items,
            limit,
            has_more,
            next_cursor,
        })
    }

    pub async fn connect(
        &self,
        caller_kind: AccountKind,
        caller_user_id: Uuid,
        command: ConnectAgent,
    ) -> ServiceResult<SpaceAgentConnection> {
        require_user_caller(caller_kind)?;
        Ok(self
            .store
            .upsert_connection(&command, caller_user_id)
            .await?)
    }

    pub async fn disconnect(
        &self,
        caller_kind: AccountKind,
        caller_user_id: Uuid,
        space_id: Uuid,
        agent_id: Uuid,
    ) -> ServiceResult<()> {
        require_user_caller(caller_kind)?;
        self.store
            .disconnect(space_id, agent_id, caller_user_id)
            .await?;
        Ok(())
    }
}

fn require_user_caller(kind: AccountKind) -> ServiceResult<()> {
    match kind {
        AccountKind::User => Ok(()),
        AccountKind::Agent => Err(ServiceError::Forbidden(
            "only user accounts may manage agent connections".to_owned(),
        )),
    }
}
