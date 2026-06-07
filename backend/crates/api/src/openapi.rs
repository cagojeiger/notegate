use axum::Router;
use notegate_core::Config;
use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};
use utoipa_swagger_ui::SwaggerUi;

use crate::state::AppState;

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::rest::files::handlers::root,
        crate::rest::files::handlers::resolve,
        crate::rest::files::handlers::children,
        crate::rest::files::handlers::create_folder,
        crate::rest::files::handlers::create_document,
        crate::rest::files::handlers::open_document,
        crate::rest::files::handlers::save_document,
        crate::rest::files::handlers::move_node,
        crate::rest::files::handlers::delete_node,
        crate::rest::files::handlers::find,
        crate::rest::files::handlers::grep,
    ),
    modifiers(&SecurityAddon),
    tags(
        (name = "files", description = "UI-oriented files API")
    )
)]
pub struct ApiDoc;

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "bearer_auth",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("JWT")
                    .build(),
            ),
        );
    }
}

pub fn routes(config: &Config) -> Router<AppState> {
    if config.openapi_enabled {
        SwaggerUi::new("/swagger-ui")
            .url("/openapi.json", ApiDoc::openapi())
            .into()
    } else {
        Router::new()
    }
}

pub fn json_pretty() -> serde_json::Result<String> {
    serde_json::to_string_pretty(&ApiDoc::openapi())
}

#[cfg(test)]
mod tests {
    use utoipa::OpenApi;

    use super::ApiDoc;

    #[test]
    fn openapi_contains_files_paths() {
        let doc = ApiDoc::openapi();
        assert!(doc.paths.paths.contains_key("/api/v1/files/root"));
        assert!(
            doc.paths
                .paths
                .contains_key("/api/v1/files/nodes/{node_id}/children")
        );
        assert!(doc.components.is_some());
    }
}
