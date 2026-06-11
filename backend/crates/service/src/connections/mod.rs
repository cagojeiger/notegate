//! Space agent connections: list / connect / disconnect.
//!
//! Users manage their own agents. A connection grants one owned agent `read` or
//! `write` permission inside one owned space.

use notegate_core::limits;
use notegate_db::ConnectionRepo;
pub use notegate_model::{ConnectAgent, ConnectionPage, ListConnections, SpaceAgentConnection};
use uuid::Uuid;

use crate::error::ServiceResult;
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
        caller_user_id: Uuid,
        space_id: Uuid,
        request: ListConnections,
    ) -> ServiceResult<ConnectionPage> {
        // Repo enforces owner-only by requiring the same user on connect/disconnect;
        // list uses the same visibility by attempting a no-op owner check through
        // live rows? For now, callers reach this through REST owner gate.
        let _ = caller_user_id;
        let limit = clamp_limit(
            request.limit,
            limits::CONNECTIONS_DEFAULT_LIMIT,
            limits::CONNECTIONS_MAX_LIMIT,
        );
        let connections = self.store.list_connections(space_id).await?;
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
        caller_user_id: Uuid,
        command: ConnectAgent,
    ) -> ServiceResult<SpaceAgentConnection> {
        Ok(self
            .store
            .upsert_connection(&command, caller_user_id)
            .await?)
    }

    pub async fn disconnect(
        &self,
        caller_user_id: Uuid,
        space_id: Uuid,
        agent_id: Uuid,
    ) -> ServiceResult<()> {
        self.store
            .disconnect(space_id, agent_id, caller_user_id)
            .await?;
        Ok(())
    }
}
