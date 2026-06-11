# Shared schemas

이 문서는 REST, MCP, future CLI가 공유하는 응답 구조를 정의한다.

## Page

```ts
type Page = {
  limit: number
  returned: number
  has_more: boolean
  next_cursor: string | null
}
```

`next_cursor`는 opaque string이다. Client는 해석하지 않고 다음 요청에 그대로 전달한다.

## TargetSelector

MCP path-first tool이 공통으로 사용하는 대상 지정자다.

```ts
type TargetSelector = {
  target?: string      // "space:/path" 축약형
  space?: string       // space name
  space_id?: string    // space UUID
  path?: string        // absolute path, 기본 "/"
}
```

`target`이 있으면 `space`, `space_id`, `path`보다 우선한다.

## NodeKind

```ts
type NodeKind = "folder" | "text" | "file"
```

## NodeTreeItem

Tree 목록, `files_ls`, `files_find`, `files_grep` 후보 결과에 사용하는 node summary다. Content body와 전체 metadata를 포함하지 않는다.

```ts
type NodeTreeItem = {
  id: string
  parent_id: string | null
  name: string
  kind: NodeKind
  path: string
  sort_order: number
  has_children: boolean
  byte_len: number | null
  updated_at: string
}
```

`byte_len`은 `folder`에서 `null`이고, `text`/`file`에서는 저장 bytes 기준이다.

## NodeDetail

`GET /nodes/{node_id}`와 `files_stat`에 사용하는 상세 node 구조다.

```ts
type NodeDetail = NodeTreeItem & {
  metadata: object
  created_at: string
  created_by_account_id: string
  updated_by_account_id: string
  text_summary?: TextSummary
  file_summary?: FileSummary
}
```

## TextSummary

```ts
type TextSummary = {
  storage_format: "plain" | "encrypted"
  content_sha256: string
  byte_len: number
  line_count: number
  media_type: string
  encoding: "utf-8"
}
```

## FileSummary

```ts
type FileSummary = {
  storage_kind: "inline_pg" | "object"
  media_type: string
  byte_len: number
  content_sha256: string
  original_filename: string | null
  encryption_mode: "none" | "client"
  encryption_metadata: object | null
}
```

## TextReadResult

```ts
type TextReadResult = {
  space: string
  node: NodeTreeItem
  storage_format: "plain" | "encrypted"
  content_sha256: string
  byte_len: number
  line_count: number
  content?: string
  encrypted_payload?: object
  unchanged?: boolean
  start_line?: number
  end_line?: number
  returned_lines?: number
  truncated?: boolean
  next_start_line?: number | null
}
```

Plain Text는 `content`를 반환한다. Encrypted Text는 `encrypted_payload`를 반환한다. `unchanged=true`이면 content body를 반환하지 않는다.
