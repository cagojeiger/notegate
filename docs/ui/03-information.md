# UI information architecture

This document maps backend resources to UI regions. REST API is the dashboard data surface.

## Backend resources

```text
Me / session        -> GET /api/v1/me
Spaces              -> GET/POST /api/v1/spaces, PATCH/DELETE /api/v1/spaces/{space_id}
Node list/create    -> GET/POST /api/v1/spaces/{space_id}/nodes
Node children       -> GET /api/v1/spaces/{space_id}/nodes/{node_id}/children
Node reveal         -> GET /api/v1/spaces/{space_id}/nodes/{node_id}/reveal
Node detail         -> GET/PATCH/DELETE /api/v1/spaces/{space_id}/nodes/{node_id}
Node metadata       -> GET/PUT/PATCH /api/v1/spaces/{space_id}/nodes/{node_id}/metadata
Text content        -> GET/PUT/PATCH /api/v1/spaces/{space_id}/text/{node_id}
File metadata       -> GET /api/v1/spaces/{space_id}/files/{node_id}
File content        -> GET /api/v1/spaces/{space_id}/files/{node_id}/content
User API keys       -> GET/POST /api/v1/me/keys, POST/DELETE /api/v1/me/keys/{key_id}
Agents              -> GET/POST /api/v1/agents, DELETE /api/v1/agents/{agent_id}
Agent API keys      -> GET/POST /api/v1/agents/{agent_id}/keys, DELETE /api/v1/agents/{agent_id}/keys/{key_id}
Agent space access  -> GET /api/v1/spaces/{space_id}/agents, PUT/DELETE /api/v1/spaces/{space_id}/agents/{agent_id}
```

## AuthScreen

Reads no workbench data.

Shows:

- login action
- auth progress/error
- developer API key fallback

Rules:

- `/api/v1/me` success enters `AppShell`.
- `/api/v1/me` 401 stays on `AuthScreen`.

## TitleBar

Shows:

- product identity
- short current space context
- layout controls

Does not show:

- current node path
- search results
- account menu

## ActivityRail

Data:

```text
GET /api/v1/spaces?limit=...&cursor=...
PATCH /api/v1/spaces/{space_id}   # reorder/name
POST /api/v1/spaces               # create
DELETE /api/v1/spaces/{space_id}  # delete
```

Shows:

- accessible spaces
- active space
- add-space action
- settings entry

Rules:

- Space create/update/delete is user-only.
- Space delete removes the space from the UI immediately after success.
- Space reorder persists `sort_order`.

## PrimarySidebar

### FilesSection

Data:

```text
GET /api/v1/spaces/{space_id}/nodes/{folder_id}/children?limit=...&cursor=...
GET /api/v1/spaces/{space_id}/nodes/{node_id}/reveal
```

Shows:

- root children as top-level rows
- folder/text/file name
- kind icon
- selected state
- has-children affordance
- pagination/loading state

Rules:

- Root `/` is not a visible row.
- Visible folders load children on demand.
- Folder row click toggles expand/collapse.
- Text/file row click opens the node in the active editor group.
- Drag/drop moves a node into a folder; it does not manually reorder siblings.
- Create actions are available for root/empty/folder contexts.
- Text/file contexts do not offer child creation.

### RecentSection

Data:

```text
GET /api/v1/spaces/{space_id}/nodes?sort=updated_at_desc&limit=...&cursor=...
```

Shows:

- recently updated nodes
- name
- path
- kind
- updated date

Rules:

- Recent is always part of `PrimarySidebar`.
- Recent uses the generic node-list API.
- Selecting a Recent row opens the node and attempts Files reveal.
- Reveal failure does not block opening the node.

## EditorArea

Data by node kind:

```text
folder -> Node detail
text   -> Node detail + Text content
file   -> Node detail + File metadata/content download
```

Shows:

- `EditorGroupHeader`: node icon, name, compact actions
- `EditorViewport`: folder/text/file/empty state

Rules:

- Header shows node name, not full path.
- Path and metrics live in Inspector.
- Text preview is default.
- Plain text renders as a simple note surface.
- Markdown renders GFM, code highlighting, and Mermaid diagrams.
- JSON/JSONL/YAML/TOML render as structured tree/source views.
- Structured tree starts expanded enough for immediate reading and supports expand/collapse-all.
- Edit mode shows line numbers.

## AuxiliarySidebar

### InspectorPanel

Data:

```text
GET /api/v1/spaces/{space_id}/nodes/{node_id}
GET /api/v1/spaces/{space_id}/nodes/{node_id}/metadata
```

Shows:

- kind
- name
- path
- node id
- created/updated attribution
- byte/line metrics
- metadata JSON
- metadata privacy note

Rules:

- Empty selection renders an empty Inspector, not a hidden/broken panel.
- Metadata is not encrypted content.
- Metadata editing uses explicit action.

### AgentPanel

Reserved for future agent context. It does not own node data.

## Settings

Tabs:

```text
Account
Agents
```

Account shows:

- current user/account
- theme control
- user API keys
- sign out

Agents shows:

- agent list with pagination
- one expanded agent at a time
- agent API keys inside the expanded agent
- agent space access inside the expanded agent

Rules:

- User API keys belong in Account.
- Agent API keys belong under the agent that owns them.
- Connections are managed inside the expanded agent, not as a separate top-level tab.
- `scopes` are not shown because the current backend policy requires empty scopes.
