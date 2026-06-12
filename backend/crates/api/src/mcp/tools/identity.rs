use axum::http::request::Parts;
use notegate_model::Caller;
use rmcp::{ErrorData, Json};

use super::resolve::invalid_input_error;
use crate::identity::me::{MeOutput, build_me};

pub fn call(parts: &Parts) -> Result<Json<MeOutput>, ErrorData> {
    let caller = parts
        .extensions
        .get::<Caller>()
        .ok_or_else(|| invalid_input_error("authenticated caller extension missing"))?;
    Ok(Json(build_me(caller)))
}
