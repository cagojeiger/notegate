# MCP Workspaces

Workspace tools let an LLM/CLI caller bootstrap and choose the workspace used by file tools.

## `workspaces_list`

List workspaces accessible to the authenticated caller.

Input:

```json
{
  "limit": 50,
  "cursor": "optional"
}
```

Branching:

```text
missing limit   -> 50
limit < 1       -> 1
limit > 100     -> 100
invalid cursor  -> invalid params
no workspaces   -> empty list; caller can use workspaces_create
```

Output:

```json
{
  "workspaces": [
    {"id": "workspace-id", "name": "personal", "role": "owner", "root_node_id": "root-node-id"}
  ],
  "page": {"limit": 50, "returned": 1, "has_more": false, "next_cursor": null}
}
```

`root_node_id` is derived from the workspace root node lookup; it is not stored on the workspace row.

## `workspaces_create`

Create a workspace owned by the authenticated caller.

Input:

```json
{"name": "personal"}
```

Branching:

```text
valid name and owned workspaces < 20 -> create workspace + root node + owner access
invalid name                         -> invalid params
owned workspaces >= 20               -> conflict
```

Output is one workspace summary:

```json
{"id": "workspace-id", "name": "personal", "role": "owner", "root_node_id": "root-node-id"}
```

## `workspaces_get`

Return one workspace by selector.

Input by name:

```json
{"workspace": "personal"}
```

Input by id:

```json
{"workspace_id": "workspace-id"}
```

Branching:

```text
one accessible match    -> workspace summary
no accessible match     -> invalid params with kind=not_found
same visible name > 1   -> invalid params with ambiguity data
workspace_id invisible  -> invalid params with kind=not_found
```
