# MCP tool contract

## 공통 규칙

- `me`와 `run_sequence`를 제외한 모든 tool은 `op`로 세부 동작을 선택한다.
- 단일 대상은 `target: "space:/absolute/path"`를 사용한다.
- 이동/복사는 `source`와 `destination`을 사용한다.
- 검색어는 `q`, 본문은 `content`, 수정 목록은 `edits`를 사용한다.
- 일반 목록은 `limit`과 opaque `cursor`를 사용한다. `changes`는 같은 opaque cursor를 `before` 또는 `after`에 넣어 방향을 선택한다.
- 동시성 guard는 `expected_sha256`, 조건부 읽기는 `if_none_match_sha256`를 사용한다.
- MCP JSON payload는 encrypted Text와 binary File bytes를 운반하지 않는다. File bytes는 `file_transfer`가 발급한 presigned URL로 직접 전송한다.
- MCP는 space create/delete/rename을 제공하지 않는다.
- `run_sequence`는 여러 command를 순서대로 실행할 때만 사용한다. rollback은 제공하지 않는다.
- 모든 입력은 알 수 없는 필드를 거부한다. `run_sequence.commands[]`는 여러 tool의 필드를 담는 공통 상위 타입이지만, 여기에 없는 필드도 거부한다.
- `target`의 Space name은 exact match이며 대소문자를 구분한다. Space name을 모르면 `read op=spaces`로 목록을 먼저 조회한다.
- Space name exact match가 실패하면 server는 case-insensitive 후보를 error `data.suggestions`에 넣을 수 있지만, 자동으로 다른 Space로 resolve하지 않는다.
- Space reconciliation 중 해당 Space의 read tool은 정상 동작하고 mutation tool은 `data.kind=usage_recalculation_in_progress`, `retryable=true`, `retry_after_seconds`를 포함한 JSON-RPC server error를 반환한다. 관리자 전체 재계산도 Space 단위로 순차 진행되므로 같은 규칙이 Space별로 적용된다. 상세 계약은 `../usage-and-quotas.md`를 따른다.

## `me`

Caller identity와 capability를 반환한다. Space 목록은 `read`의 `op=spaces`로 조회한다.

## `read`

Read-only tool이다.

```ts
type ReadInput = {
  op: "spaces" | "ls" | "tree" | "stat" | "read" | "changes"
  target?: string
  name?: string
  depth?: number
  limit?: number
  cursor?: string
  before?: string
  after?: string
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
| 최신 변경 확인 | `before`, `after` 모두 생략 | `event_id DESC` | `page.next.before` |
| 더 오래된 변경 탐색 | `before=<cursor>` | `event_id DESC` | `page.next.before` |
| cursor 이후 변경 적용 | `after=<cursor>` | `event_id ASC` | `page.next.after` 또는 `applied_cursor` |

`event_id`는 하나의 mutation event 식별자이며 Space 안에서는 큰 값이 더 최신이다. `created_at`은 표시 시각이고 정본 순서는 `event_id`다. `before`와 `after`는 동시에 사용할 수 없다. Changes cursor는 Space에 묶인 opaque 값이므로 내부 event id를 만들거나 다른 Space에 재사용하지 않는다.

`changes`는 operation filter 없이 Folder/Text/File의 create, content/metadata update, move, copy, delete, write-lock 변경을 모두 반환한다. move/delete의 subtree 경계를 놓치지 않도록 target은 `<space>:/` Space root만 허용한다.

새 캐시는 **`changes(limit=1)`에서 `head_cursor` 저장 → 현재 Space snapshot 구성 → 저장한 cursor를 `after`로 조회** 순서로 시작한다. 마지막 조회가 snapshot을 읽는 동안 발생한 변경을 회수한다. 응답의 event를 `event_id ASC` 순서대로 모두 적용한 뒤에만 `applied_cursor`를 저장한다. `page.has_more=true`이면 `page.next.after`로 계속 읽는다. `resync_required=true`이면 cursor 이후의 연속성을 보장할 수 없으므로 현재 Space tree를 다시 만들고 `next_action.new_head_cursor`에서 재개한다.

응답은 방향과 무관하게 `page: {limit, returned, has_more, start_cursor, end_cursor, next}`를 사용한다. 최신/과거 조회의 `next`는 `{before}`, 이후 변경 조회의 `next`는 `{after}`다. `after` 응답의 `next_action`은 다음 상태를 구조화한다.

- `call_tool`: 같은 `limit`과 새 `after` cursor로 다음 page를 호출한다.
- `store_cursor`: 반환된 event를 모두 적용한 뒤 `applied_cursor`를 저장한다.
- `resync_required`: 현재 Space snapshot을 다시 구성하고 새 `head_cursor`에서 재개한다.

결정적으로 복구할 수 있는 잘못된 입력은 JSON-RPC error의 `data`에 `code`, `recoverable`, `hint`, `next_action`을 반환한다. 같은 입력의 단순 재시도는 성공하지 않으므로 `retryable=false`이며, 호출자는 자연어 message를 분석하지 않고 `code`와 `next_action`으로 수정한다.

| code | 원인 | `next_action.kind` |
| --- | --- | --- |
| `changes_direction_conflict` | `before`와 `after` 동시 사용 | `choose_direction` |
| `changes_cursor_field_invalid` | changes에 일반 `cursor` 사용 | `choose_cursor_field` |
| `changes_fields_not_allowed` | 다른 op/tool에 `before`/`after` 사용 | `remove_fields` |
| `changes_scope_invalid` | Space root가 아닌 target | `replace_field` |
| `changes_cursor_invalid` | 손상되거나 다른 형식의 cursor | 과거 탐색은 `call_tool`, 이후 변경은 `rebuild_snapshot` |
| `changes_cursor_scope_mismatch` | 다른 Space cursor | 과거 탐색은 `call_tool`, 이후 변경은 `rebuild_snapshot` |

`events[]`는 `event_id`, `created_at`, `node_id`, `actor_account_id`, `operation`, `metadata`, `item_kind`, `affected_parent_ids`와 `path_changed`, `subtree_changed`, `write_lock_changed` 영향 flag를 반환한다. `parent_scope_known=false`인 이전 event는 정확한 parent 범위를 알 수 없으므로 보수적으로 현재 상태를 다시 조회한다.

Node summary의 `write_locked`는 대상에 직접 설정된 잠금, `effective_write_locked`는 직접 또는 상속 잠금의 적용 여부다. `op=stat`은 현재 쓰기를 막는 직접 잠금 source를 `write_lock_sources[]`의 `node_id`, `name`, `path`로 함께 반환한다.

필수 필드:

```text
spaces: op
ls:     op, target
tree:   op, target
stat:   op, target
read:   op, target
changes: op, target, mode
```

## `search`

Read-only search tool이다.

```ts
type SearchInput = {
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
- process 동시성 상한을 넘으면 `data.code=search_busy`, `operation=find|grep`,
  `retryable=true`, `retry_after_ms=1000`을 반환한다. `run_sequence`의 search command도
  같은 제한을 사용한다.

필수 필드:

```text
find: op, target, q
grep: op, target, q
```

Traversal, cursor, memory budget은 [`../search.md`](../search.md)를 따른다.

## `write`

Plain Text content를 생성하거나 수정한다. Folder 이동/삭제는 하지 않는다.

```ts
type WriteInput = {
  op: "write" | "append" | "patch" | "edit"
  target: string
  content?: string
  edits?: unknown[]
  create?: boolean
  ensure_newline?: boolean
  expected_sha256?: string
}
```

- `op=write`: 전체 content replacement다. 없으면 `create=true`가 필요하다.
- `op=append`: EOF append다. `ensure_newline=true`이면 기존 content가 비어 있지 않고 newline으로 끝나지 않을 때 content 앞에 newline을 넣는다.
- `op=patch`: string replacement다. edit entry는 `old_text`, `new_text`, optional `mode: "unique"|"first"|"all"`, optional `expected_count`를 가진다.
- `op=edit`: 1-based line operation이다. `insert_before_line`, `insert_after_line`, `replace_lines`, `delete_lines`를 지원한다. insert/replace `content`는 논리적인 줄 내용으로 해석되며 trailing newline이 없어도 줄 경계를 보존한다. `content`는 여러 줄을 포함할 수 있다.
- `.json`, `.jsonl`, `.yaml`, `.yml`, `.toml` Text는 service layer의 공통 규칙으로 저장 전에 문법 검증한다. 검증은 target path의 file name extension 기준이며 schema validation은 하지 않는다.

필수 필드:

```text
write:  op, target, content
append: op, target, content
patch:  op, target, edits
edit:   op, target, edits
```

## `manage`

기존 Space 내부의 tree/location을 변경한다. Space lifecycle은 제공하지 않는다.

```ts
type ManageInput = {
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

필수 필드:

```text
mkdir: op, target
mv:    op, source, destination
cp:    op, source, destination
rm:    op, target
```

## `file_transfer`

로컬 caller와 S3 호환 저장소 사이의 직접 File 전송을 준비한다. Caller는 응답의 presigned URL을 전송에만 사용하고 로그나 문서에 지속 저장하지 않는다. API key는 transfer 응답에 포함되지 않는다.

```ts
type FileTransferInput = {
  op: "begin_upload" | "prepare_parts" | "complete_upload" | "abort_upload" | "prepare_download"
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

- `begin_upload`: 새 File target과 byte length를 검증하고 upload handle을 만든다. 100MiB 이하는 single PUT URL을, 초과하면 `part_size`와 `part_count`를 반환한다.
- `prepare_parts`: multipart part 번호를 최대 16개까지 받아 5분짜리 PUT URL을 발급한다. Caller는 part를 최대 4개까지 병렬 업로드하고 실패한 part만 새 URL로 다시 시도한다. 호출할 때마다 무활동 정리 시각을 갱신한다.
- `complete_upload`: single object 또는 모든 multipart ETag를 검증하고 File node를 연결한다.
- `abort_upload`: 완료되지 않은 upload를 비동기 정리 대상으로 전환한다.
- `prepare_download`: 기존 File target의 5분짜리 GET URL을 반환한다.

필수 필드:

```text
begin_upload:     op, target, byte_len
prepare_parts:    op, upload_id, part_numbers
complete_upload:  op, upload_id (+ multipart는 completed_parts)
abort_upload:     op, upload_id
prepare_download: op, target
```

File bytes는 MCP request/response에 포함하지 않는다. Single/multipart PUT의 성공 응답 ETag는 로컬 caller가 수집해 multipart complete에 전달한다. `file_transfer`는 외부 전송 사이에 caller 작업이 필요하므로 `run_sequence` 안에서 실행할 수 없다.

모든 성공 응답은 `next_action`을 포함한다. `kind=call_tool`은 `tool`과 `input`을 다음 MCP 호출에 사용하고, `kind=http_upload|http_upload_parts|http_download`는 지정된 `transfer_field` 또는 `transfers_field`의 URL과 header로 로컬 HTTP 전송을 수행한다. `kind=done`은 추가 단계가 없다는 뜻이다. Multipart PUT은 `max_concurrency=4` 이하로 병렬 전송하고 `collect_response_header=etag`에 따라 각 응답 ETag를 수집한다. 실패한 part는 `repeat`에 따라 새 URL을 준비해 다시 전송하고, 모든 part가 끝나면 `then`에 따라 `{part_number, etag}`를 `complete_upload.completed_parts`로 전달한다.

완료되지 않은 upload는 `begin_upload`, `prepare_parts`, `complete_upload` 중 마지막 활동 이후 2시간이 지나면 비동기 정리 대상이 된다. `begin_upload`는 destination의 write lock을 검사하고 해당 handle의 write-lock 허가를 예약한다. 이후 destination이 잠겨도 기존 upload handle은 완료할 수 있지만 새 `begin_upload`는 거부한다. 완료 시 일반 write permission과 File 생성 invariant는 다시 확인한다. Presigned URL의 5분 만료와 upload 원장의 2시간 무활동 만료는 서로 다른 제한이다.

Write command가 잠금으로 거부되면 MCP error `data`는 `kind=write_locked`, `retryable=false`와 다음 code 중 하나를 포함한다.

- `node_write_locked`: target 또는 조상에 직접 잠금이 있다. `read op=stat`의 `write_lock_sources`로 source를 확인한다.
- `subtree_write_locked`: rename/move/delete 대상 subtree 안에 직접 잠금이 있다.

잠금 변경은 MCP tool이 제공하지 않으며 Space owner가 Dashboard에서 수행한다.

## `run_sequence`

여러 NoteGate command를 순서대로 실행한다. 단일 command는 `read`, `search`, `write`, `manage`를 직접 호출한다.

```ts
type RunSequenceInput = {
  commands: SequenceCommand[] // 1..20
}

type SequenceCommand = {
  tool: "read" | "search" | "write" | "manage"
  op: string
  target?: string
  source?: string
  destination?: string
  name?: string
  q?: string
  kind?: "folder" | "text" | "file"
  match?: string
  lines?: "none" | "first" | "all"
  include?: string[]
  exclude?: string[]
  content?: string
  edits?: unknown[]
  create?: boolean
  parents?: boolean
  recursive?: boolean
  ensure_newline?: boolean
  depth?: number
  limit?: number
  cursor?: string
  before?: string
  after?: string
  start_line?: number
  max_lines?: number
  max_bytes?: number
  expected_sha256?: string
  if_none_match_sha256?: string
}
```

Semantics:

- `commands`는 입력 순서대로 실행한다.
- 각 command는 기존 `read`/`search`/`write`/`manage`와 같은 validation, permission, service transaction을 사용한다.
- 각 command의 필수 필드는 해당 tool의 필수 필드를 따른다.
- `SequenceCommand`는 공통 상위 타입이다. 해당 op가 사용하지 않는 known 필드는 실행 입력으로 전달되지 않는다.
- command 하나가 실패하면 즉시 중단한다.
- 이미 성공한 command는 rollback하지 않는다.
- `run_sequence` 안에서 `run_sequence`를 다시 호출할 수 없다.
- 결과는 성공한 command의 결과와 실패 위치를 반환한다.

```json
{
  "ok": false,
  "completed": 2,
  "failed_index": 2,
  "results": [
    { "index": 0, "tool": "manage", "op": "mkdir", "ok": true, "result": {} },
    { "index": 1, "tool": "write", "op": "write", "ok": true, "result": {} }
  ],
  "error": {
    "code": -32602,
    "message": "...",
    "data": { "kind": "invalid_input", "code": "invalid_input" }
  }
}
```
