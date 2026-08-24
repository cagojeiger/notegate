mod admission;
mod agent_text;
mod auth;
mod background_jobs;
mod boot;
mod command_api;
mod commands;
mod error;
mod file_change;
mod file_preview;
mod identity;
mod internal_search;
mod mcp;
mod metadata_write_behind;
mod object_storage;
mod object_upload_flow;
mod observability;
mod openapi;
mod page;
mod path_node_summary;
mod periodic_worker;
mod process_runtime;
mod public_v2;
mod reconciliations;
mod rest;
mod routes;
mod runtime_plan;
mod state;
mod usage_bootstrap;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    boot::run().await
}
