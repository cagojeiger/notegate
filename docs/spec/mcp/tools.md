# MCP tool contract

## 공통 호출 규칙

| 개념 | 입력 |
| --- | --- |
| 세부 동작 | `me`, `file_download`, `run_read_sequence`, `run_write_sequence`를 제외한 tool의 `op` |
| 호출 목적 | `me`를 제외한 최상위 `purpose` 하나. 앞뒤 공백 없는 1..200자이며 secret, 본문, 검색 결과는 제외 |
| 단일 대상 | `target: "space:/absolute/path"` |
| 이동·복사 | `source`, `destination` |
| 검색·본문·수정 | `q`, `content`, `edits` |
| pagination | `limit`, opaque `cursor`, 응답의 `page.next_cursor`; `changes`는 `direction=older|newer` 추가 |
| 조건부 처리 | write guard `expected_sha256`, read guard `if_none_match_sha256` |

- Operation 하나는 직접 tool을 호출한다.
- 목적과 입력이 정해진 여러 read/search 또는 write/manage command는 1..20개짜리 sequence로 묶는다.
- 앞 응답의 cursor, sha256 또는 target이 다음 입력에 필요하면 직접 tool을 단계별로 호출한다.

Sequence의 `commands[]`는 최상위 `purpose`를 상속하므로 개별 `purpose`나 `args` wrapper를 넣지 않는다.

- 모든 입력은 unknown field를 거부하며 sequence도 `tool`별 직접 tool schema를 사용한다.
- Space name은 exact, case-sensitive로 resolve한다. 이름을 모르면 `read op=spaces`를 먼저 호출한다. 실패 응답은 case-insensitive `data.suggestions`를 제공할 수 있으며 server가 후보를 자동 선택하지 않는다.
- MCP는 Space 내부 content를 다루고 REST/dashboard가 Space lifecycle을 담당한다.
- Encrypted Text와 File bytes는 MCP JSON payload 밖에 둔다. File bytes는 presigned URL로 전송한다.
- `run_write_sequence`의 완료된 mutation은 유지된다. File tool은 sequence 밖에서 호출한다.
- Space reconciliation 중에는 read가 계속 동작하고 mutation은 `data.kind=usage_recalculation_in_progress`, `retryable=true`, `retry_after_seconds`를 반환한다. 상세 계약은 [`usage-and-quotas.md`](../usage-and-quotas.md)를 따른다.

## `me`

Caller identity, capability, 실행 중인 `server_version`을 반환한다. Space 목록은 `read`의 `op=spaces`로 조회한다.

## `read`

Read-only tool이다.

```ts
type ReadInput = {
  purpose: string
  op: "spaces" | "ls" | "tree" | "stat" | "read" | "changes"
  target?: string
  name?: string
  depth?: number
  limit?: number
  cursor?: string
  direction?: "older" | "newer"
  start_line?: number
  max_lines?: number
  max_bytes?: number
  if_none_match_sha256?: string
}
```

- `op=spaces`: 접근 가능한 Space 목록을 반환한다. `name`이 있으면 exact, case-sensitive name으로 조회한다.
- `op=ls`: `target` folder의 direct children을 반환한다.
- `op=tree`: `target` folder의 subtree를 DFS pre-order로 반환한다. `depth` 생략 시 5를 사용한다.
- `op=stat`: Folder/Text/File node summary를 반환한다.
- `op=read`: plain Text content를 읽는다. line/byte range를 지원한다.
- `op=changes`: Space 전체의 mutation change-event stream을 읽는다. 모든 방향에서 같은 `events[]`와 안정적인 `event_id` 체계를 반환한다.

### `changes` 방향 선택

| 목적 | 입력 | 순서 | 이어서 읽는 값 |
| --- | --- | --- | --- |
| 최신 변경 확인 | `direction`과 `cursor` 생략 | `event_id DESC` | `page.next_cursor` |
| 더 오래된 변경 탐색 | `direction=older`, `cursor=<cursor>` | `event_id DESC` | `page.next_cursor` |
| cursor 이후 변경 적용 | `direction=newer`, `cursor=<cursor>` | `event_id ASC` | `page.next_cursor` 또는 `checkpoint_cursor` |

`direction`의 기본값은 `older`이며 `newer`는 checkpoint `cursor`가 필수다. `event_id`는 하나의 mutation event 식별자이며 Space 안에서는 큰 값이 더 최신이다. `created_at`은 표시 시각이고 정본 순서는 `event_id`다. Changes cursor는 Space에 묶인 opaque 값이므로 내부 event id를 만들거나 다른 Space에 재사용하지 않는다.

`changes`는 operation filter 없이 Folder/Text/File의 create, content update, move, copy, delete, write-lock 변경을 모두 반환한다. move/delete의 subtree 경계를 놓치지 않도록 target은 `<space>:/` Space root만 허용한다.

`changes.events[].metadata`는 변경 event의 구조적 payload이고 `file_upload.encryption_metadata`는 client-side encryption 계약이다. Node metadata mutation은 MCP command surface에 포함되지 않는다.

Changes 호출 이력의 input redaction과 `space_name` summary는 [`event-logging.md`](../event-logging.md#command-invocation-history)를 따른다.

새 cache는 다음 순서로 동기화한다.

1. `changes(limit=1)`의 `checkpoint_cursor`를 기준점으로 잡는다.
2. 현재 Space snapshot을 만든다.
3. `direction=newer, cursor=<checkpoint_cursor>`로 snapshot 생성 중 발생한 변경을 회수한다.

Event는 `event_id ASC` 순서대로 적용하고 해당 page를 모두 반영한 뒤에만 새 `checkpoint_cursor`를 저장한다. `page.has_more=true`이면 `page.next_cursor`로 계속 읽는다. `resync_required=true`이면 Space snapshot을 다시 만들고 응답의 `checkpoint_cursor`에서 재개한다.

응답은 다른 paginated read/search와 동일하게 `page: {limit, returned, has_more, next_cursor}`를 사용한다. `direction=newer` 응답의 `next_action`은 다음 상태를 구조화한다.

- `call_tool`: 같은 `limit`, `direction=newer`, 새 `cursor`로 다음 page를 호출한다.
- `store_cursor`: 반환된 event를 모두 적용한 뒤 `checkpoint_cursor`를 저장한다.
- `rebuild_snapshot`: 현재 Space snapshot을 다시 구성하고 새 `checkpoint_cursor`에서 재개한다.

복구 가능한 입력 오류는 `retryable=false`와 구조화된 `code`, `recoverable`, `hint`, `next_action`을 반환한다.

| code | 원인 | `next_action.kind` |
| --- | --- | --- |
| `changes_direction_invalid` | `older`, `newer`가 아닌 방향 사용 | `choose_value` |
| `changes_cursor_required` | `direction=newer`에 cursor 누락 | `rebuild_snapshot` |
| `changes_fields_not_allowed` | 다른 op/tool에 `direction` 사용 | `remove_fields` |
| `changes_scope_invalid` | Space root가 아닌 target | `replace_field` |
| `changes_cursor_invalid` | 손상되거나 다른 형식의 cursor | 과거 탐색은 `call_tool`, 이후 변경은 `rebuild_snapshot` |
| `changes_cursor_scope_mismatch` | 다른 Space cursor | 과거 탐색은 `call_tool`, 이후 변경은 `rebuild_snapshot` |

`events[]`는 `event_id`, `created_at`, `node_id`, `actor_account_id`, `operation`, `metadata`, `item_kind`, `affected_parent_ids`와 `path_changed`, `subtree_changed`, `write_lock_changed` 영향 flag를 반환한다. `parent_scope_known=false`인 event는 정확한 parent 범위를 알 수 없으므로 보수적으로 현재 상태를 다시 조회한다.

Node summary의 `write_locked`는 대상에 직접 설정된 잠금, `effective_write_locked`는 직접 또는 상속 잠금의 적용 여부다. `op=stat`은 현재 쓰기를 막는 직접 잠금 source를 `write_lock_sources[]`의 `node_id`, `name`, `path`로 함께 반환한다.

필수 입력은 `spaces`의 `purpose, op`, 나머지 read operation의 `purpose, op, target`이다.

## `search`

Read-only search tool이다.

```ts
type SearchInput = {
  purpose: string
  op: "find" | "grep"
  target: string
  q: string
  kind?: "folder" | "text" | "file"
  match?: string
  lines?: "none" | "first" | "all"
  include?: string[]
  exclude?: string[]
  limit?: number
  cursor?: string
}
```

- `op=find`: node name을 검색한다. `match`는 `contains`(기본), `regex`, `glob`이다.
- `op=grep`: plain Text content를 검색한다. `match`는 `literal`(기본), `regex`이다.
- `find`와 `grep` match는 Space 내부에서 대소문자를 구분하지 않는다.
- `include`/`exclude`는 결과 path에 적용하는 glob list다.
- `grep lines=none`은 line 정보를 반환하지 않는다. `first`는 첫 matching line number, `all`은 모든 matching line number를 반환한다. snippet은 반환하지 않는다.
- File, client-side encrypted Text, metadata는 `grep` 대상이 아니다. 서버 관리 방식으로 at-rest 암호화된 plain Text는 복호화 후 검색한다.

| `data.code` | 조건 | 복구 |
| --- | --- | --- |
| `search_busy` | process 동시성 상한 초과 | `operation=find|grep`, `retryable=true`, `retry_after_ms=1000`; sequence에도 같은 상한 적용 |
| `search_unavailable` | Search role 연결·서명·private response 검증 실패 | `retryable=true`, `retry_after_ms=1000` |
| `deadline_exceeded` | 외부 요청 deadline 소진 | `retryable=true`; target scope나 limit을 줄여 새 요청 |

JSON-RPC server code `-32001`은 여러 임시 dependency 오류가 공유하므로 caller는 `data.code`로 분기한다. Search는 같은 요청을 내부에서 자동 재시도하지 않는다.

Traversal, cursor, memory budget은 [`../search.md`](../search.md)를 따른다.

## `write`

Plain Text content를 생성하거나 수정한다. Folder 이동/삭제는 하지 않는다.

```ts
type WriteInput = {
  purpose: string
  op: "write" | "append" | "patch" | "edit"
  target: string
  content?: string
  edits?: Array<PatchEdit | LineEditInput>
  create?: boolean
  ensure_newline?: boolean
  expected_sha256?: string
}

type PatchEdit = {
  old_text: string
  new_text: string
  mode?: "unique" | "first" | "all"
  expected_count?: number
}

type LineEditInput = {
  op: "insert_before_line" | "insert_after_line" | "replace_lines" | "delete_lines"
  line?: number
  start_line?: number
  end_line?: number
  content?: string
}
```

- `op=write`: 전체 content replacement다. 없으면 `create=true`가 필요하다.
- `op=append`: EOF append다. `ensure_newline=true`이면 기존 content가 비어 있지 않고 newline으로 끝나지 않을 때 content 앞에 newline을 넣는다.
- `op=patch`: `edits[]`에 `PatchEdit`만 받는 string replacement다.
- `op=edit`: `edits[]`에 `LineEditInput`만 받는 1-based line operation이다. insert/replace `content`는 논리적인 줄 내용으로 해석되며 trailing newline이 없어도 줄 경계를 보존한다. `content`는 여러 줄을 포함할 수 있다.
- `.json`, `.jsonl`, `.yaml`, `.yml`, `.toml` Text는 service layer의 공통 규칙으로 저장 전에 문법 검증한다. 검증은 target path의 file name extension 기준이며 schema validation은 하지 않는다.

`write`와 `append`는 `purpose, op, target, content`, `patch`와 `edit`은 `purpose, op, target, edits`가 필수다.

## `manage`

Space 내부의 tree/location을 변경한다. Space lifecycle은 제공하지 않는다.

```ts
type ManageInput = {
  purpose: string
  op: "mkdir" | "mv" | "cp" | "rm"
  target?: string
  source?: string
  destination?: string
  parents?: boolean
  recursive?: boolean
}
```

- `op=mkdir`: `target` folder를 만든다. `parents=true`이면 `mkdir -p`처럼 missing parent를 생성한다.
- `op=mv`: `source` node를 `destination`으로 이동/rename한다. 같은 Space 안에서만 가능하다.
- `op=cp`: `source` node를 `destination`으로 복사한다. Folder copy는 `recursive=true`가 필요하다.
- `op=rm`: `target` node를 soft-delete한다. Folder delete는 `recursive=true`가 필요하다.

Space root(`<space>:/`)는 `op=mkdir, parents=true`의 idempotent target으로만 허용한다. 그 외 `target`, `source`, `destination`은 반드시 node를 가리켜야 한다.

`mkdir`와 `rm`은 `purpose, op, target`, `mv`와 `cp`는 `purpose, op, source, destination`이 필수다.

## `file_download`

한 File target의 5분짜리 presigned GET URL을 반환한다.

```ts
type FileDownloadInput = {
  purpose: string
  target: string
}
```

File bytes는 MCP payload를 통과하지 않는다. Caller는 `next_action.kind=http_download`의 `transfer_field`를 따라 로컬에서 다운로드하고 URL이나 header를 로그·문서에 저장하지 않는다.

## `file_upload`

S3 호환 저장소로 직접 업로드하는 lifecycle을 제공한다.

```ts
type FileUploadInput = {
  purpose: string
  op: "begin_upload" | "prepare_parts" | "complete_upload" | "abort_upload"
  target?: string
  byte_len?: number
  media_type?: string
  original_filename?: string
  encryption_mode?: "none" | "client"
  encryption_metadata?: object
  upload_id?: string
  part_numbers?: number[]
  completed_parts?: { part_number: number, etag: string }[]
}
```

- `begin_upload`: target과 byte length를 검증하고 upload handle을 만든다.
- `prepare_parts`: 최대 16개 multipart part URL과 `max_concurrency`를 반환한다.
- `complete_upload`: object 또는 multipart ETag를 검증하고 File node를 연결한다.
- `abort_upload`: 완료되지 않은 upload를 비동기 정리 대상으로 전환한다.

모든 operation에 `purpose, op`가 필요하며 나머지 필수 입력은 다음과 같다.

| Operation | 필수 입력 |
| --- | --- |
| `begin_upload` | `target`, `byte_len` |
| `prepare_parts` | `upload_id`, `part_numbers` |
| `complete_upload` | `upload_id`; multipart는 `completed_parts` 추가 |
| `abort_upload` | `upload_id` |

모든 성공 응답은 `next_action`을 포함한다. Multipart PUT은 응답의 `max_concurrency` 이하로 병렬 전송하고 각 ETag를 수집해 `complete_upload.completed_parts`로 전달한다. 완료되지 않은 upload의 정리와 URL lifetime은 [`performance-limits.md`](../performance-limits.md), write-lock 규칙은 [`files-commands.md`](../files-commands.md#write-lock)를 따른다.

File tool은 외부 HTTP 전송 사이에 caller 작업이 필요하므로 sequence에 포함할 수 없다.

## Sequence tools

두 sequence 모두 최상위 `purpose`와 1..20개의 flat command를 받는다. 실행 전에 모든 command를 검증한다.

하나라도 잘못되면 아무 command도 실행하지 않고 `executed=false`, 빈 `results`, command별 `errors[]`와 `next_action.kind=apply_error_actions`를 error data에 반환한다.

### `run_read_sequence`

`read`와 `search` command만 받는다. 최대 4개를 병렬 실행하고 모든 성공·실패 결과를 입력 index 순서로 반환한다.

```ts
type RunReadSequenceInput = {
  purpose: string
  commands: Array<
    | ({ tool: "read" } & Omit<ReadInput, "purpose">)
    | ({ tool: "search" } & Omit<SearchInput, "purpose">)
  >
}
```

### `run_write_sequence`

`write`와 `manage` command만 받는다. 입력 순서대로 한 번에 하나씩 실행하며 첫 runtime 실패에서 중단한다. 완료된 mutation은 유지되고 실행하지 않은 command 수는 `skipped`로 반환된다.

```ts
type RunWriteSequenceInput = {
  purpose: string
  commands: Array<
    | ({ tool: "write" } & Omit<WriteInput, "purpose">)
    | ({ tool: "manage" } & Omit<ManageInput, "purpose">)
  >
}
```

공통 runtime 응답은 다음 형태다.

```json
{
  "ok": false,
  "completed": 1,
  "failed": 1,
  "skipped": 2,
  "results": [
    { "index": 0, "tool": "write", "op": "write", "ok": true, "result": {} },
    {
      "index": 1,
      "tool": "manage",
      "op": "rm",
      "ok": false,
      "error": { "code": -32602, "message": "...", "data": { "code": "invalid_input" } }
    }
  ]
}
```

개별 command는 직접 tool과 같은 validation, permission, write-lock, service transaction을 사용한다. command 내부에는 `purpose`나 `args`를 넣지 않는다. sequence tool이나 File tool을 중첩할 수 없다.

Write command가 잠금으로 거부되면 MCP error `data`는 `kind=write_locked`, `retryable=false`와 `node_write_locked` 또는 `subtree_write_locked` code를 포함한다. 잠금 변경은 MCP가 제공하지 않으며 Space owner가 Dashboard에서 수행한다.

## 공통 error와 후속 action

Tool error `data`는 공통 분류 `kind`와 안정적인 `code`를 사용하며 caller는 자연어 `message` 대신 `kind`, `code`, `data.next_action`으로 분기한다. 입력을 구조적으로 수정할 수 있으면 `retryable=false`, `recoverable=true`, `hint`, `next_action`을 함께 반환한다. 성공 응답의 후속 동작도 같은 tagged action 계약을 사용한다.

```ts
type McpAction =
  | { kind: "add_fields"; fields: Array<{ field: string; description?: string }> }
  | { kind: "remove_fields"; fields: string[] }
  | { kind: "replace_field"; field: string; value: unknown }
  | { kind: "choose_value"; field: string; choices: unknown[] }
  | { kind: "apply_error_actions"; errors_field: string }
  | { kind: "call_tool"; tool: string; input: object; reason?: string; instruction?: string }
  | { kind: "rebuild_snapshot"; reason?: string; cursor?: string; baseline_call?: ToolCallSpec }
  | { kind: "store_cursor"; reason: string; cursor: string }
  | { kind: "http_upload"; transfer_field: string; instruction: string; then: ToolCallSpec }
  | { kind: "http_upload_parts"; /* multipart transfer instructions */ }
  | { kind: "http_download"; transfer_field: string; instruction: string }
  | { kind: "done" }

type ToolCallSpec = { tool: string; input: object }
```

공개 tool schema의 공통 필수 입력은 handler 실행 전에 검증한다. 누락 시 `code=required_fields_missing`, `next_action.kind=add_fields`를 반환한다. `op`별 필수 입력 누락은 `code=required_field_missing`과 같은 action을 사용한다. `fields[].description`에 맞춰 입력을 보완해 같은 tool을 다시 호출한다.

실행된 Sequence command의 runtime error는 해당 `results[]` 항목의 `error: {code, message, data}`에 들어가며 직접 tool과 같은 복구 분기를 사용한다.
