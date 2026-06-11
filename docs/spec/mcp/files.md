# MCP Files

MCP files tools는 Space tree를 path-first로 다룬다. 공통 schema는 `../schemas.md`를 따른다.

## Target selector

`TargetSelector`는 `../schemas.md`를 따른다.

## `files_ls`

Folder children을 조회한다.

```ts
type FilesLsInput = TargetSelector & {
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

## `files_stat`

Folder/Text/File 상태를 조회한다. Node metadata는 MCP stat 응답에 포함하지 않는다.

```ts
type FilesStatInput = TargetSelector

type FilesStatOutput = {
  space: string
  node: McpNodeSummary
}
```

## `files_mkdir`

Folder를 생성한다.

```ts
type FilesMkdirInput = TargetSelector

type FilesMkdirOutput = {
  space: string
  node: McpNodeSummary
}
```

`path` 또는 `target`은 생성할 folder 경로다. Parent folder는 이미 존재해야 한다.

## `files_touch`

빈 plain Text node를 생성한다.

```ts
type FilesTouchInput = TargetSelector

type FilesTouchOutput = {
  space: string
  node: McpNodeSummary
  content_sha256: string
  byte_len: number
  line_count: number
}
```

`path` 또는 `target`은 생성할 Text 경로다. Parent folder는 이미 존재해야 한다.

## `files_read`

Plain Text content를 읽는다.

```ts
type FilesReadInput = TargetSelector & {
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
type FilesWriteInput = TargetSelector & {
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
type FilesPatchInput = TargetSelector & {
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
  space?: string
  space_id?: string
  source_path: string
  destination_path: string
}

type FilesMvOutput = {
  space: string
  node: McpNodeSummary
}
```

Space 간 move는 지원하지 않는다.

## `files_rm`

Node를 soft delete한다.

```ts
type FilesRmInput = TargetSelector & {
  recursive?: boolean
}

type FilesRmOutput = {
  space: string
  node_id: string
  path: string
  deleted: true
  purge_after: string
}
```

Folder 삭제는 `recursive=true`가 필요하다.

## File content

MCP upload/download tool은 제공하지 않는다. File은 `files_ls`/`files_find`에서 `McpNodeSummary`로 확인하고 `files_stat`에서 file stats를 확인한다. File은 `files_read`/`files_write`/`files_patch`/`files_grep` 대상이 아니다.
