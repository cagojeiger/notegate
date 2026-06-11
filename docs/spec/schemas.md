# Shared schemas

이 문서는 node/text/file surface가 공유하는 응답 구조를 정의한다. REST는 UI 화면 렌더링을 위해 id-first의 넓은 resource shape를 반환하고, MCP는 CLI/agent 호출을 위해 path-first의 작은 tool shape를 반환한다.

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
  space_id?: string    // space UUID fallback
  path?: string        // absolute path, 기본 "/"
}
```

`target`이 있으면 `space`, `space_id`, `path`보다 우선한다.

## NodeKind

```ts
type NodeKind = "folder" | "text" | "file"
```

## AccountRef

```ts
type AccountRef = {
  id: string
  kind: "user" | "agent"
  display_name: string
}
```

## NodeRef

REST Text response처럼 이미 선택된 node를 짧게 참조할 때 사용한다.

```ts
type NodeRef = {
  id: string
  path: string
  kind: NodeKind
}
```

## RestNode

REST node/file endpoints가 반환하는 UI용 node resource shape다. Content body는 포함하지 않는다.

```ts
type RestNode = {
  id: string
  space_id: string
  parent_id: string | null
  name: string
  kind: NodeKind
  path: string
  sort_order: number
  metadata: object
  has_children: boolean

  // text 또는 file node에서만 존재
  content_sha256?: string
  byte_len?: number

  // text node에서만 존재
  line_count?: number

  // file node에서만 존재
  storage_kind?: "inline_pg" | "object"
  media_type?: string
  original_filename?: string
  encryption_mode?: "none" | "client"
  encryption_metadata?: object

  created_by: AccountRef
  updated_by: AccountRef
  created_at: string
  updated_at: string
}
```

`byte_len`은 Text에서는 저장된 text payload 기준이고 File에서는 저장 bytes 기준이다. Folder에는 `byte_len`이 없다.

## McpNodeSummary

MCP `files_ls`, `files_stat`, `files_find`, `files_grep`가 반환하는 path-first node summary다. Content body와 node metadata는 포함하지 않는다.

```ts
type McpNodeSummary = {
  node_id: string
  path: string
  name: string
  kind: NodeKind
  sort_order: number
  has_children: boolean
  created_at: string
  updated_at: string

  // text 또는 file node에서만 존재
  content_sha256?: string
  byte_len?: number

  // text node에서만 존재
  line_count?: number

  // file node에서만 존재
  storage_kind?: "inline_pg" | "object"
  media_type?: string
  original_filename?: string
  encryption_mode?: "none" | "client"
  encryption_metadata?: object
}
```

## Text read shapes

REST Text read는 `{ node: NodeRef, text: ... }` envelope를 사용한다. MCP `files_read`는 path-first flat shape를 사용한다.

```ts
type RestTextReadResponse = {
  node: NodeRef
  text: RestTextReadBody
}

type RestTextReadBody =
  | RestPlainTextRead
  | RestEncryptedTextRead
  | RestTextUnchanged

type RestPlainTextRead = {
  node_id: string
  storage_format: "plain"
  content: string
  content_sha256: string
  byte_len: number
  line_count: number
  start_line: number
  end_line: number
  returned_lines: number
  truncated: boolean
  next_start_line: number | null
  updated_by: AccountRef
  updated_at: string
}

type RestEncryptedTextRead = {
  node_id: string
  storage_format: "encrypted"
  encrypted_payload: object
  content_sha256: string
  byte_len: number
  line_count: 0
  updated_by: AccountRef
  updated_at: string
}

type RestTextUnchanged = {
  node_id: string
  storage_format: "plain" | "encrypted"
  unchanged: true
  content_returned: false
  content_sha256: string
  byte_len: number
  line_count: number
}
```

```ts
type McpTextReadResult =
  | {
      space: string
      path: string
      content: string
      content_sha256: string
      byte_len: number
      line_count: number
      start_line: number
      end_line: number
      returned_lines: number
      truncated: boolean
      next_start_line: number | null
    }
  | {
      space: string
      path: string
      unchanged: true
      content_returned: false
      content_sha256: string
    }
```

MCP는 encrypted Text를 읽지 않는다. Encrypted Text는 REST Text API에서만 저장/조회한다.
