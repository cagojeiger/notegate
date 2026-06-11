# MCP Search

MCP search는 Space path를 기준으로 folder subtree를 탐색한다. 공통 traversal, cursor, memory budget은 `../search.md`를 따른다. 공통 schema는 `../schemas.md`를 따른다.

## `files_find`

Node name을 검색한다.

```ts
type FilesFindInput = TargetSelector & {
  q: string
  kind?: "folder" | "text" | "file"
  match?: "contains" | "regex" | "glob"
  limit?: number
  cursor?: string
}

type FilesFindOutput = {
  space: string
  items: NodeTreeItem[]
  page: Page
}
```

기본 `match`는 `contains`다. Folder/Text/File 모두 대상이다. Root node는 결과에서 제외한다. Content와 metadata는 검색하지 않는다.

예:

```json
{"target":"personal:/notes","q":"state","kind":"text","match":"contains","limit":50}
```

## `files_grep`

Query를 포함하는 plain Text node 후보를 검색한다.

```ts
type FilesGrepInput = TargetSelector & {
  q: string
  match?: "literal" | "regex"
  include?: string[]
  exclude?: string[]
  limit?: number
  cursor?: string
}

type FilesGrepOutput = {
  space: string
  items: NodeTreeItem[]
  page: Page
}
```

기본 `match`는 `literal`이다. `include`/`exclude`는 path glob list이며 각 list는 최대 32개 pattern, pattern 하나는 최대 256자다. 응답은 match line이 아니라 Text node 후보 목록이다. File, encrypted Text, metadata는 대상이 아니다. Match된 Text의 내용은 `files_read`로 조회한다.

예:

```json
{"target":"personal:/memory","q":"todo","match":"literal","limit":20}
```
