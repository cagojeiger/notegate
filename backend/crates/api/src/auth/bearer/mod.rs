pub mod error;
pub mod extractor;
mod verify;

pub(crate) use error::auth_error_response;
pub use error::{
    AuthError, auth_error_body, map_identity_error, shared_scoped_challenge_header,
    status_for_error,
};
pub use extractor::{extract_bearer, extract_cookie_value};
pub use verify::verify_bearer_mcp;
