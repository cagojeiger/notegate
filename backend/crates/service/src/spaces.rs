//! Space lifecycle: create / get / rename / delete.
//!
//! Spaces are user-owned. The owner user has implicit write permission; agents
//! only see spaces through explicit connections.

use notegate_core::limits;
use notegate_core::validation::validate_space_name;
use notegate_db::SpaceRepo;
use notegate_model::{AccountKind, Permission};
pub use notegate_model::{CreateSpace, ListSpaces, SpaceCursor, SpacePage, SpaceView, UpdateSpace};
use uuid::Uuid;

use crate::cursor;
use crate::error::{ServiceError, ServiceResult};
use crate::pagination::clamp_limit;

#[derive(Debug, Clone)]
pub struct SpaceService {
    store: SpaceRepo,
}

impl SpaceService {
    pub fn new(store: SpaceRepo) -> Self {
        Self { store }
    }

    pub async fn create(
        &self,
        caller_kind: AccountKind,
        caller_account_id: Uuid,
        command: CreateSpace,
    ) -> ServiceResult<SpaceView> {
        require_user_caller(caller_kind)?;
        validate_space_name(&command.name)?;

        let space = self.store.create_space(caller_account_id, &command).await?;
        let root_node_id =
            self.store.root_node_id(space.id).await?.ok_or_else(|| {
                ServiceError::Internal("space root node was not created".to_owned())
            })?;

        Ok(SpaceView {
            space,
            permission: Permission::Write,
            root_node_id,
        })
    }

    pub async fn get(&self, caller_account_id: Uuid, space_id: Uuid) -> ServiceResult<SpaceView> {
        self.store
            .find_space_view_for(caller_account_id, space_id)
            .await?
            .ok_or_else(not_found)
    }

    pub async fn list(
        &self,
        caller_account_id: Uuid,
        request: ListSpaces,
    ) -> ServiceResult<SpacePage> {
        let limit = clamp_limit(
            request.limit,
            limits::SPACES_DEFAULT_LIMIT,
            limits::SPACES_MAX_LIMIT,
        );
        let cursor = match request.cursor.as_deref() {
            None => None,
            Some(raw) => Some(
                cursor::decode::<SpaceCursor>(raw)
                    .map_err(|_error| ServiceError::InvalidInput("invalid cursor".to_owned()))?,
            ),
        };

        let mut items = self
            .store
            .list_space_views_for(caller_account_id, limit + 1, cursor.as_ref())
            .await?;
        let has_more = items.len() as i64 > limit;
        items.truncate(limit as usize);
        let next_cursor = if has_more {
            items
                .last()
                .map(|view| SpaceCursor {
                    sort_order: view.space.sort_order,
                    name: view.space.name.clone(),
                    id: view.space.id,
                })
                .map(|cursor| cursor::encode(&cursor))
                .transpose()
                .map_err(|_error| ServiceError::Internal("failed to encode cursor".to_owned()))?
        } else {
            None
        };

        Ok(SpacePage {
            items,
            limit,
            has_more,
            next_cursor,
        })
    }

    pub async fn find_visible_by_id(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
    ) -> ServiceResult<Option<SpaceView>> {
        Ok(self
            .store
            .find_space_view_for(caller_account_id, space_id)
            .await?)
    }

    pub async fn find_visible_by_name(
        &self,
        caller_account_id: Uuid,
        name: &str,
        limit: i64,
    ) -> ServiceResult<Vec<SpaceView>> {
        validate_space_name(name)?;
        let limit = clamp_limit(Some(limit), 1, limits::SPACES_MAX_LIMIT);
        Ok(self
            .store
            .list_space_views_by_name_for(caller_account_id, name, limit)
            .await?)
    }

    pub async fn find_visible_by_name_case_insensitive(
        &self,
        caller_account_id: Uuid,
        name: &str,
        limit: i64,
    ) -> ServiceResult<Vec<SpaceView>> {
        validate_space_name(name)?;
        let limit = clamp_limit(Some(limit), 1, limits::SPACES_MAX_LIMIT);
        Ok(self
            .store
            .list_space_views_by_name_case_insensitive_for(caller_account_id, name, limit)
            .await?)
    }

    pub async fn update(
        &self,
        caller_kind: AccountKind,
        caller_account_id: Uuid,
        command: UpdateSpace,
    ) -> ServiceResult<SpaceView> {
        require_user_caller(caller_kind)?;
        if command.name.is_none() && command.sort_order.is_none() {
            return Err(ServiceError::InvalidInput(
                "provide name and/or sort_order to update".to_owned(),
            ));
        }
        if let Some(name) = command.name.as_deref() {
            validate_space_name(name)?;
        }
        self.require_write(command.space_id, caller_account_id)
            .await?;

        let space = self
            .store
            .update_space(
                command.space_id,
                caller_account_id,
                command.name.as_deref(),
                command.sort_order,
            )
            .await?;
        let root_node_id = self
            .store
            .root_node_id(command.space_id)
            .await?
            .ok_or_else(not_found)?;

        Ok(SpaceView {
            space,
            permission: Permission::Write,
            root_node_id,
        })
    }

    pub async fn delete(
        &self,
        caller_kind: AccountKind,
        caller_account_id: Uuid,
        space_id: Uuid,
    ) -> ServiceResult<()> {
        require_user_caller(caller_kind)?;
        self.require_write(space_id, caller_account_id).await?;
        self.store
            .delete_space(space_id, caller_account_id, caller_account_id)
            .await?;
        Ok(())
    }

    async fn require_permission(
        &self,
        space_id: Uuid,
        account_id: Uuid,
    ) -> ServiceResult<Permission> {
        self.store
            .permission_for(space_id, account_id)
            .await?
            .ok_or_else(not_found)
    }

    async fn require_write(&self, space_id: Uuid, account_id: Uuid) -> ServiceResult<()> {
        let permission = self.require_permission(space_id, account_id).await?;
        if !permission.allows_write() {
            return Err(ServiceError::Forbidden(
                "write permission required".to_owned(),
            ));
        }
        Ok(())
    }
}

fn require_user_caller(kind: AccountKind) -> ServiceResult<()> {
    match kind {
        AccountKind::User => Ok(()),
        AccountKind::Agent => Err(ServiceError::Forbidden(
            "only user accounts may manage spaces".to_owned(),
        )),
    }
}

fn not_found() -> ServiceError {
    ServiceError::NotFound("space not found".to_owned())
}
