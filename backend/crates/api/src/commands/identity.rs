//! Transport-neutral identity command.

use schemars::JsonSchema;
use serde::Serialize;

use super::CommandContext;
use crate::identity::me::{MeOutput, build_me};

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct IdentityOutput {
    #[serde(flatten)]
    pub identity: MeOutput,
    /// Version of the running NoteGate server binary.
    pub server_version: String,
}

impl IdentityOutput {
    pub fn new(identity: MeOutput) -> Self {
        Self {
            identity,
            server_version: env!("CARGO_PKG_VERSION").to_owned(),
        }
    }
}

pub fn call(context: &CommandContext) -> IdentityOutput {
    IdentityOutput::new(build_me(context.caller()))
}
