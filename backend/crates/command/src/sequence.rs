use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    MANAGE_OPERATIONS, ManageInput, ManageOperationSchema, READ_OPERATIONS, ReadInput,
    ReadOperationSchema, SEARCH_OPERATIONS, SearchOperationSchema, WRITE_OPERATIONS,
    WriteEditEntrySchema, WriteOperationSchema,
};

pub const SEQUENCE_MAX_COMMANDS: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceKind {
    Read,
    Write,
}

impl SequenceKind {
    pub fn tool_name(self) -> &'static str {
        match self {
            Self::Read => "run_read_sequence",
            Self::Write => "run_write_sequence",
        }
    }

    pub fn allowed_tools(self) -> &'static [&'static str] {
        match self {
            Self::Read => &["read", "search"],
            Self::Write => &["write", "manage"],
        }
    }

    pub fn operation_help(self) -> String {
        match self {
            Self::Read => format!(
                "read={}; search={}.",
                READ_OPERATIONS.join("/"),
                SEARCH_OPERATIONS.join("/")
            ),
            Self::Write => format!(
                "write={}; manage={}.",
                WRITE_OPERATIONS.join("/"),
                MANAGE_OPERATIONS.join("/")
            ),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SequenceCommand {
    pub tool: String,
    pub op: String,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub destination: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default, rename = "match")]
    pub match_mode: Option<String>,
    #[serde(default)]
    pub lines: Option<String>,
    #[serde(default)]
    pub include: Option<Vec<String>>,
    #[serde(default)]
    pub exclude: Option<Vec<String>>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub edits: Option<Vec<Value>>,
    #[serde(default)]
    pub create: bool,
    #[serde(default)]
    pub parents: bool,
    #[serde(default)]
    pub recursive: bool,
    #[serde(default)]
    pub ensure_newline: bool,
    #[serde(default)]
    pub depth: Option<i64>,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub direction: Option<String>,
    #[serde(default)]
    pub start_line: Option<i64>,
    #[serde(default)]
    pub max_lines: Option<i64>,
    #[serde(default)]
    pub max_bytes: Option<usize>,
    #[serde(default)]
    pub expected_sha256: Option<String>,
    #[serde(default)]
    pub if_none_match_sha256: Option<String>,
}

impl SequenceCommand {
    pub fn into_read_input(self, purpose: String) -> ReadInput {
        ReadInput {
            purpose,
            op: self.op,
            target: self.target,
            name: self.name,
            depth: self.depth,
            limit: self.limit,
            cursor: self.cursor,
            direction: self.direction,
            start_line: self.start_line,
            max_lines: self.max_lines,
            max_bytes: self.max_bytes,
            if_none_match_sha256: self.if_none_match_sha256,
        }
    }

    pub fn into_manage_input(self, purpose: String) -> ManageInput {
        ManageInput {
            purpose,
            op: self.op,
            target: self.target,
            source: self.source,
            destination: self.destination,
            parents: self.parents,
            recursive: self.recursive,
        }
    }
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(tag = "tool", rename_all = "snake_case")]
#[schemars(inline)]
enum ReadSequenceCommandSchema {
    Read(SequenceReadCommandSchema),
    Search(SequenceSearchCommandSchema),
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(inline)]
struct SequenceReadCommandSchema {
    op: ReadOperationSchema,
    target: Option<String>,
    name: Option<String>,
    depth: Option<i64>,
    limit: Option<i64>,
    cursor: Option<String>,
    direction: Option<String>,
    start_line: Option<i64>,
    max_lines: Option<i64>,
    max_bytes: Option<usize>,
    if_none_match_sha256: Option<String>,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(inline)]
struct SequenceSearchCommandSchema {
    op: SearchOperationSchema,
    target: String,
    q: String,
    kind: Option<String>,
    #[serde(rename = "match")]
    match_mode: Option<String>,
    lines: Option<String>,
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    limit: Option<i64>,
    cursor: Option<String>,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(tag = "tool", rename_all = "snake_case")]
#[schemars(inline)]
enum WriteSequenceCommandSchema {
    Write(SequenceWriteCommandSchema),
    Manage(SequenceManageCommandSchema),
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(inline)]
struct SequenceWriteCommandSchema {
    op: WriteOperationSchema,
    target: String,
    content: Option<String>,
    #[schemars(with = "Option<Vec<WriteEditEntrySchema>>")]
    edits: Option<Vec<Value>>,
    #[serde(default)]
    create: bool,
    #[serde(default)]
    ensure_newline: bool,
    expected_sha256: Option<String>,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(inline)]
struct SequenceManageCommandSchema {
    op: ManageOperationSchema,
    target: Option<String>,
    source: Option<String>,
    destination: Option<String>,
    #[serde(default)]
    parents: bool,
    #[serde(default)]
    recursive: bool,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunReadSequenceInput {
    /// Reason for this command invocation. Required once at the top level; commands inherit it and must not include purpose; maximum 200 characters.
    pub purpose: String,
    /// Read/search command objects. Each includes tool and op, omits purpose and args; 1..20.
    #[schemars(with = "Vec<ReadSequenceCommandSchema>", length(min = 1, max = 20))]
    pub commands: Vec<Value>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunWriteSequenceInput {
    /// Reason for this command invocation. Required once at the top level; commands inherit it and must not include purpose; maximum 200 characters.
    pub purpose: String,
    /// Write/manage command objects. Each includes tool and op, omits purpose and args; 1..20.
    #[schemars(with = "Vec<WriteSequenceCommandSchema>", length(min = 1, max = 20))]
    pub commands: Vec<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemars::schema_for;
    use serde_json::json;

    #[test]
    fn sequence_contract_requires_one_top_level_purpose() {
        assert!(
            serde_json::from_value::<RunReadSequenceInput>(json!({
                "commands": [{"tool": "read", "op": "spaces"}]
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<RunWriteSequenceInput>(json!({
                "purpose": "create notes",
                "commands": [{"tool": "manage", "op": "mkdir", "target": "daily:/notes"}]
            }))
            .is_ok()
        );
    }

    #[test]
    fn sequence_schemas_expose_the_same_flat_tool_union_to_every_transport()
    -> Result<(), serde_json::Error> {
        let read = serde_json::to_value(schema_for!(RunReadSequenceInput))?;
        let write = serde_json::to_value(schema_for!(RunWriteSequenceInput))?;

        let read_text = read.to_string();
        let write_text = write.to_string();
        assert!(read_text.contains("read"));
        assert!(read_text.contains("search"));
        assert!(write_text.contains("write"));
        assert!(write_text.contains("manage"));
        Ok(())
    }
}
