# MCP Files

MCP files tools는 Space tree를 path-first로 다룬다. 공통 schema는 `../schemas.md`를 따른다.

## Target

MCP file tool은 `target: "space:/path"` 하나로 Space와 path를 함께 지정한다.

## `files_ls`

Folder children을 조회한다.

```ts
type FilesLsInput = {
  target: string
  limit?: number
  cursor?: string
}

type FilesLsOutput = {
  space: string
  path: string
  children: McpNodeSummary[]
  page: Page
}
```


## `files_tree`

Folder subtree를 DFS pre-order로 조회한다. `files_ls`는 direct children 전용이고, `files_tree`는 depth 기반 구조 조회다.

```ts
type FilesTreeInput = {
  target: string
  depth?: number
  limit?: number
  cursor?: string
}

type FilesTreeOutput = {
  space: string
  path: string
  depth: number
  items: McpNodeSummary[]
  page: Page
}
```

기본 `depth`는 2, 최소 1, 최대 Space path depth다. `depth=1`은 선택 folder의 direct children만 반환한다. 순서는 DFS pre-order이며 sibling order는 `sort_order ASC, name ASC` 뒤 내부 tie-breaker로 안정화한다.

## `files_stat`

Folder/Text/File 상태를 조회한다. Node metadata는 MCP stat 응답에 포함하지 않는다.

```ts
type FilesStatInput = {
  target: string
}

type FilesStatOutput = {
  space: string
  node: McpNodeSummary
}
```

## `files_mkdir`

Folder를 생성한다.

```ts
type FilesMkdirInput = {
  target: string
  parents?: boolean
}

type FilesMkdirOutput = {
  space: string
  node: McpNodeSummary
  created_paths?: string[]
}
```

`target`은 생성할 folder 경로다. `parents=false` 또는 생략이면 parent folder는 이미 존재해야 한다. `parents=true`이면 missing parent folder를 순서대로 생성한다. 이미 존재하는 folder는 통과하고, 중간 경로에 Text/File이 있으면 conflict다.

## `files_touch`

빈 plain Text node를 생성한다.

```ts
type FilesTouchInput = {
  target: string
}

type FilesTouchOutput = {
  space: string
  node: McpNodeSummary
  content_sha256: string
  byte_len: number
  line_count: number
}
```

`target`은 생성할 Text 경로다. Parent folder는 이미 존재해야 한다.

## `files_read`

Plain Text content를 읽는다.

```ts
type FilesReadInput = {
  target: string
  start_line?: number
  max_lines?: number
  max_bytes?: number
  if_none_match_sha256?: string
}

type FilesReadOutput = McpTextReadResult
```

Encrypted Text와 File은 `files_read` 대상이 아니다.

## `files_write`

Plain Text content 전체를 쓴다.

```ts
type FilesWriteInput = {
  target: string
  content: string
  create?: boolean
  expected_sha256?: string
}

type FilesWriteOutput = {
  space: string
  node: McpNodeSummary
  content_sha256: string
  byte_len: number
  line_count: number
}
```

`create=true`이면 없을 때 Text node를 생성한다. MCP는 encrypted Text write를 제공하지 않는다.

## `files_patch`

Plain Text exact-match patch를 적용한다.

```ts
type FilesPatchInput = {
  target: string
  edits: { old_text: string, new_text: string }[]
  expected_sha256?: string
}

type FilesPatchOutput = {
  space: string
  path: string
  patched: true
  edits_applied: number
  content_sha256: string
  previous_sha256: string
  byte_len: number
  line_count: number
  diff: string
}
```

각 `old_text`는 원문에서 정확히 한 번만 매칭되어야 한다.

## `files_mv`

Node를 rename/move한다.

```ts
type FilesMvInput = {
  source: string
  destination: string
}

type FilesMvOutput = {
  space: string
  node: McpNodeSummary
}
```

`source`와 `destination`은 같은 Space여야 한다. Space 간 move는 지원하지 않는다.

## `files_rm`

Node를 soft delete한다.

```ts
type FilesRmInput = {
  target: string
  recursive?: boolean
}

type FilesRmOutput = {
  space: string
  path: string
  deleted: true
  purge_after: string
}
```

Folder 삭제는 `recursive=true`가 필요하다.

## File content

MCP upload/download tool은 제공하지 않는다. File은 `files_ls`/`files_find`에서 `McpNodeSummary`로 확인하고 `files_stat`에서 file stats를 확인한다. File은 `files_read`/`files_write`/`files_patch`/`files_grep` 대상이 아니다.
