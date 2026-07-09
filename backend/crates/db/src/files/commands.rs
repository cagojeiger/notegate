//! Mutating commands for the file tree.
pub mod checks;
pub mod copy_node;
pub mod create;
pub mod delete;
pub mod move_node;
pub mod save;
pub mod update;

fn stored_text_parts(
    content: &notegate_model::files::StoredContent,
) -> (&'static str, Option<&str>, Option<&serde_json::Value>) {
    match &content.body {
        notegate_model::files::WriteTextBody::Plain(content) => {
            ("plain", Some(content.as_str()), None)
        }
        notegate_model::files::WriteTextBody::Encrypted(payload) => {
            ("encrypted", None, Some(payload))
        }
    }
}
