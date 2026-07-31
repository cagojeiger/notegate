use notegate_model::Caller;
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Serialize, ToSchema)]
pub struct MeResponse {
    pub account: AccountOut,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AccountOut {
    pub id: Uuid,
    pub kind: String,
    pub display_name: String,
}

impl From<&Caller> for MeResponse {
    fn from(caller: &Caller) -> Self {
        Self {
            account: AccountOut {
                id: caller.account.id,
                kind: caller.account.kind.as_str().to_owned(),
                display_name: caller.account.display_name.clone(),
            },
        }
    }
}
