# MCP Workspaces

Workspace tool은 LLM/CLI caller가 파일 tool에서 사용할 workspace를 조회, 생성, 선택하기 위한 전역 tool이다.

## `workspaces_list`

인증된 caller가 접근 가능한 workspace 목록을 반환한다.

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
no workspaces   -> empty list; user caller는 workspaces_create를 사용할 수 있고, agent caller는 owner의 access grant가 필요하다
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

`root_node_id`는 workspace row에 저장하지 않고 workspace root node lookup으로 derive한다.

## `workspaces_create`

인증된 user caller가 소유하는 workspace를 생성한다. Agent caller는 workspace를 생성할 수 없다.

Input:

```json
{"name": "personal"}
```

Branching:

```text
user caller and owned workspaces < 20 -> create workspace + root node + owner access
agent caller                          -> invalid request with data.kind=forbidden
invalid name                          -> invalid params
owned workspaces >= 20                -> conflict
```

Output은 workspace summary 하나다.

```json
{"id": "workspace-id", "name": "personal", "role": "owner", "root_node_id": "root-node-id"}
```

## `workspaces_get`

Selector로 accessible workspace 하나를 반환한다.

Input by name:

```json
{"workspace": "personal"}
```

Input by id:

```json
{"workspace_id": "workspace-id"}
```

Input omitted:

```json
{}
```

Branching:

```text
workspace_id visible              -> workspace summary
workspace name matches exactly one -> workspace summary
selector omitted and exactly one   -> workspace summary
selector omitted and zero visible  -> invalid params; user는 workspaces_create 가능, agent는 access grant 필요
selector omitted and many visible  -> invalid params; pass workspace
no accessible name match           -> invalid params with data.kind=not_found
same visible name > 1              -> invalid params with ambiguity data
workspace_id invisible             -> invalid params with data.kind=not_found
```
