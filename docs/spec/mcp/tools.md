# MCP tool contract

## 공통 규칙

- `me`와 `run_sequence`를 제외한 모든 tool은 `op`로 세부 동작을 선택한다.
- 하나의 MCP 호출은 `me`를 제외하고 최상위 `purpose`를 정확히 하나 받는다. 앞뒤 공백 없는 1..200자이며 secret, 본문, 검색 결과를 넣지 않는다.
- `run_sequence.commands[]`는 별도 MCP 호출이 아니라 내부 command다. 최상위 호출의 `purpose`를 상속하므로 개별 `purpose`를 넣지 않는다.
- operation 하나는 직접 tool을 호출한다. 입력이 미리 정해지고 목적이 같은 operation 2..20개는 `run_sequence`를 우선한다. 뒤 입력에 앞 응답의 cursor, sha256, 발견된 target 등이 필요하면 직접 tool을 단계별로 호출한다.
- 단일 대상은 `target: "space:/absolute/path"`를 사용한다.
- 이동/복사는 `source`와 `destination`을 사용한다.
- 검색어는 `q`, 본문은 `content`, 수정 목록은 `edits`를 사용한다.
- 모든 paginated read/search는 `limit`, opaque `cursor`, 응답의 `page.next_cursor`를 사용한다. `changes`만 `direction=older|newer`로 진행 방향을 선택한다.
- 기존 node mutation의 동시성 guard는 `expected_revision`이다. Text는 선택적 `expected_sha256`을 추가로 사용할 수 있고 조건부 읽기는 `if_none_match_sha256`를 사용한다.
- MCP JSON payload는 encrypted Text와 binary File bytes를 운반하지 않는다. File bytes는 `file_transfer`가 발급한 presigned URL로 직접 전송한다.
- MCP는 space create/delete/rename을 제공하지 않는다.
- `run_sequence`의 완료된 mutation은 rollback하지 않는다. `file_transfer`는 sequence에 포함할 수 없다.
- 모든 입력은 알 수 없는 필드를 거부한다. `run_sequence.commands[]`는 `tool`별 branch가 해당 직접 tool의 op와 필드만 노출한다.
- `target`의 Space name은 exact match이며 대소문자를 구분한다. Space name을 모르면 `read op=spaces`로 목록을 먼저 조회한다.
- Space name exact match가 실패하면 server는 case-insensitive 후보를 error `data.suggestions`에 넣을 수 있지만, 자동으로 다른 Space로 resolve하지 않는다.
- Space reconciliation 중 해당 Space의 read tool은 정상 동작하고 mutation tool은 `data.kind=usage_recalculation_in_progress`, `retryable=true`, `retry_after_seconds`를 포함한 JSON-RPC server error를 반환한다. 관리자 전체 재계산도 Space 단위로 순차 진행되므로 같은 규칙이 Space별로 적용된다. 상세 계약은 `../usage-and-quotas.md`를 따른다.
- Tool handler가 반환하는 MCP error `data`는 공통 분류 `kind`와 안정적인 `code`를 사용한다. 호출자가 자연어 message를 해석하지 않고 입력을 수정할 수 있는 경우에는 `retryable=false`, `recoverable=true`, `hint`, `next_action`을 추가한다.

### 공통 action과 error

성공 응답의 후속 동작과 복구 가능한 error의 `data.next_action`은 같은 tagged action 계약을 사용한다. 호출자는 설명 문구 대신 `kind`로 분기한다.

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

공개 tool schema의 필수 입력이 빠지면 server는 tool handler 실행 전에 `code=required_fields_missing`과 `next_action.kind=add_fields`를 반환한다. 통합 tool의 `op`별 선택 필드가 빠지면 `code=required_field_missing`과 같은 action을 반환한다. `fields[].description`이 있으면 그 설명에 맞는 값을 구성해 같은 tool을 다시 호출한다.

`run_sequence`에서 한 command가 실패하면 `error: {code, message, data}`에 동일한 error data를 넣는다. 따라서 직접 호출과 sequence 내부 호출의 복구 분기가 같다.

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

`changes`는 operation filter 없이 Folder/Text/File의 create, content/metadata update, move, copy, delete, write-lock 변경을 모두 반환한다. move/delete의 subtree 경계를 놓치지 않도록 target은 `<space>:/` Space root만 허용한다.

호출 이력에는 `tools/call.params.arguments` 원본을 JSON으로 저장한다. 따라서 changes target도 `input`에서 확인할 수 있고, 목록용으로 검증된 Space 이름을 `space_name` summary에 함께 남긴다.

새 캐시는 **`changes(limit=1)`에서 `checkpoint_cursor` 저장 → 현재 Space snapshot 구성 → `direction=newer, cursor=<checkpoint_cursor>`로 조회** 순서로 시작한다. 마지막 조회가 snapshot을 읽는 동안 발생한 변경을 회수한다. 응답의 event를 `event_id ASC` 순서대로 모두 적용한 뒤에만 새 `checkpoint_cursor`를 저장한다. `page.has_more=true`이면 `page.next_cursor`로 계속 읽는다. `resync_required=true`이면 cursor 이후의 연속성을 보장할 수 없으므로 현재 Space tree를 다시 만들고 응답의 `checkpoint_cursor`에서 재개한다.

응답은 다른 paginated read/search와 동일하게 `page: {limit, returned, has_more, next_cursor}`를 사용한다. `direction=newer` 응답의 `next_action`은 다음 상태를 구조화한다.

- `call_tool`: 같은 `limit`, `direction=newer`, 새 `cursor`로 다음 page를 호출한다.
- `store_cursor`: 반환된 event를 모두 적용한 뒤 `checkpoint_cursor`를 저장한다.
- `rebuild_snapshot`: 현재 Space snapshot을 다시 구성하고 새 `checkpoint_cursor`에서 재개한다.

결정적으로 복구할 수 있는 잘못된 입력은 JSON-RPC error의 `data`에 `code`, `recoverable`, `hint`, `next_action`을 반환한다. 같은 입력의 단순 재시도는 성공하지 않으므로 `retryable=false`이며, 호출자는 자연어 message를 분석하지 않고 `code`와 `next_action`으로 수정한다.

| code | 원인 | `next_action.kind` |
| --- | --- | --- |
| `changes_direction_invalid` | `older`, `newer`가 아닌 방향 사용 | `choose_value` |
| `changes_cursor_required` | `direction=newer`에 cursor 누락 | `rebuild_snapshot` |
| `changes_fields_not_allowed` | 다른 op/tool에 `direction` 사용 | `remove_fields` |
| `changes_scope_invalid` | Space root가 아닌 target | `replace_field` |
| `changes_cursor_invalid` | 손상되거나 다른 형식의 cursor | 과거 탐색은 `call_tool`, 이후 변경은 `rebuild_snapshot` |
| `changes_cursor_scope_mismatch` | 다른 Space cursor | 과거 탐색은 `call_tool`, 이후 변경은 `rebuild_snapshot` |

`events[]`는 `event_id`, `created_at`, `node_id`, `actor_account_id`, `operation`, `metadata`, `item_kind`, `affected_parent_ids`와 `path_changed`, `subtree_changed`, `write_lock_changed` 영향 flag를 반환한다. `parent_scope_known=false`인 이전 event는 정확한 parent 범위를 알 수 없으므로 보수적으로 현재 상태를 다시 조회한다.

Node summary의 `write_locked`는 대상에 직접 설정된 잠금, `effective_write_locked`는 직접 또는 상속 잠금의 적용 여부다. `op=stat`은 현재 쓰기를 막는 직접 잠금 source를 `write_lock_sources[]`의 `node_id`, `name`, `path`로 함께 반환한다.

필수 필드:

```text
spaces: purpose, op
ls:     purpose, op, target
tree:   purpose, op, target
stat:   purpose, op, target
read:   purpose, op, target
changes: purpose, op, target
```

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
- process 동시성 상한을 넘으면 `data.code=search_busy`, `operation=find|grep`,
  `retryable=true`, `retry_after_ms=1000`을 반환한다. `run_sequence`의 search command도
  같은 제한을 사용한다.

필수 필드:

```text
find: purpose, op, target, q
grep: purpose, op, target, q
```

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
  expected_revision?: number
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
- 기존 Text의 `write`/`append`와 모든 `patch`/`edit`은 `expected_revision`이 필수다. `create=true`로 새 Text를 만들 때는 생략한다.
- `.json`, `.jsonl`, `.yaml`, `.yml`, `.toml` Text는 service layer의 공통 규칙으로 저장 전에 문법 검증한다. 검증은 target path의 file name extension 기준이며 schema validation은 하지 않는다.

필수 필드:

```text
write:  purpose, op, target, content
append: purpose, op, target, content
patch:  purpose, op, target, edits
edit:   purpose, op, target, edits
```

## `manage`

기존 Space 내부의 tree/location을 변경한다. Space lifecycle은 제공하지 않는다.

```ts
type ManageInput = {
  purpose: string
  op: "mkdir" | "mv" | "cp" | "rm"
  target?: string
  source?: string
  destination?: string
  parents?: boolean
  recursive?: boolean
  expected_revision?: number
}
```

- `op=mkdir`: `target` folder를 만든다. `parents=true`이면 `mkdir -p`처럼 missing parent를 생성한다.
- `op=mv`: `source` node를 `destination`으로 이동/rename한다. 같은 Space 안에서만 가능하다.
- `op=cp`: `source` node를 `destination`으로 복사한다. Folder copy는 `recursive=true`가 필요하다.
- `op=rm`: `target` node를 soft-delete한다. Folder delete는 `recursive=true`가 필요하다.
- `op=mv`, `op=rm`은 source/target의 최신 `expected_revision`이 필수다. `mkdir`, `cp`는 새 node를 만들므로 생략한다.

Space root(`<space>:/`)는 `op=mkdir, parents=true`의 idempotent target으로만 허용한다. 그 외 `target`, `source`, `destination`은 반드시 node를 가리켜야 한다.

필수 필드:

```text
mkdir: purpose, op, target
mv:    purpose, op, source, destination
cp:    purpose, op, source, destination
rm:    purpose, op, target
```

## `file_transfer`

로컬 caller와 S3 호환 저장소 사이의 직접 File 전송을 준비한다. Caller는 응답의 presigned URL을 전송에만 사용하고 로그나 문서에 지속 저장하지 않는다. API key는 transfer 응답에 포함되지 않는다.

```ts
type FileTransferInput = {
  purpose: string
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
begin_upload:     purpose, op, target, byte_len
prepare_parts:    purpose, op, upload_id, part_numbers
complete_upload:  purpose, op, upload_id (+ multipart는 completed_parts)
abort_upload:     purpose, op, upload_id
prepare_download: purpose, op, target
```

File bytes는 MCP request/response에 포함하지 않는다. Single/multipart PUT의 성공 응답 ETag는 로컬 caller가 수집해 multipart complete에 전달한다. `file_transfer`는 외부 전송 사이에 caller 작업이 필요하므로 `run_sequence` 안에서 실행할 수 없다.

모든 성공 응답은 `next_action`을 포함한다. `kind=call_tool`은 `tool`과 `input`을 다음 MCP 호출에 사용하며, 이 input에는 현재 호출과 같은 `purpose`가 포함된다. `kind=http_upload|http_upload_parts|http_download`는 지정된 `transfer_field` 또는 `transfers_field`의 URL과 header로 로컬 HTTP 전송을 수행한다. `kind=done`은 추가 단계가 없다는 뜻이다. Multipart PUT은 `max_concurrency=4` 이하로 병렬 전송하고 `collect_response_header=etag`에 따라 각 응답 ETag를 수집한다. 실패한 part는 `repeat`에 따라 새 URL을 준비해 다시 전송하고, 모든 part가 끝나면 `then`에 따라 `{part_number, etag}`를 `complete_upload.completed_parts`로 전달한다.

완료되지 않은 upload는 `begin_upload`, `prepare_parts`, `complete_upload` 중 마지막 활동 이후 2시간이 지나면 비동기 정리 대상이 된다. `begin_upload`는 destination의 write lock을 검사하고 해당 handle의 write-lock 허가를 예약한다. 이후 destination이 잠겨도 기존 upload handle은 완료할 수 있지만 새 `begin_upload`는 거부한다. 완료 시 일반 write permission과 File 생성 invariant는 다시 확인한다. Presigned URL의 5분 만료와 upload 원장의 2시간 무활동 만료는 서로 다른 제한이다.

Write command가 잠금으로 거부되면 MCP error `data`는 `kind=write_locked`, `retryable=false`와 다음 code 중 하나를 포함한다.

- `node_write_locked`: target 또는 조상에 직접 잠금이 있다. `read op=stat`의 `write_lock_sources`로 source를 확인한다.
- `subtree_write_locked`: rename/move/delete 대상 subtree 안에 직접 잠금이 있다.

잠금 변경은 MCP tool이 제공하지 않으며 Space owner가 Dashboard에서 수행한다.

## `run_sequence`

여러 NoteGate command를 순서대로 실행한다. 단일 command는 `read`, `search`, `write`, `manage`를 직접 호출한다.

```ts
type RunSequenceInput = {
  purpose: string
  commands: SequenceCommand[] // 1..20
}

type SequenceCommand =
  | ({ tool: "read" } & Omit<ReadInput, "purpose">)
  | ({ tool: "search" } & Omit<SearchInput, "purpose">)
  | ({ tool: "write" } & Omit<WriteInput, "purpose">)
  | ({ tool: "manage" } & Omit<ManageInput, "purpose">)
```

```json
{
  "purpose": "Read two known notes",
  "commands": [
    { "tool": "read", "op": "read", "target": "daily:/one.md" },
    { "tool": "read", "op": "read", "target": "daily:/two.md" }
  ]
}
```

Semantics:

- 공개 JSON Schema는 `tool`로 구분되는 네 command branch를 제공한다. 각 branch는 해당 직접 tool의 op와 필드만 노출한다.
- 실행 전에 모든 command의 구조, 필수 필드, operation, target 형식과 요청만으로 판단 가능한 본문 제한 및 구조화 `write` 문법을 preflight한다. 하나라도 잘못되면 아무 command도 실행하지 않고 error `data`에 `ok=false`, `phase=preflight`, `executed=false`, `completed=0`, `failed_index=null`, 빈 `results`, command별 `errors[]`와 `next_action`을 반환한다. 최상위 `next_action.kind=apply_error_actions`는 각 `errors[].next_action`을 적용하라는 뜻이다.
- command는 `tool`, `op`, operation 필드를 직접 담는 flat object다. 개별 command에 `purpose`를 반복하거나 `args`로 감싸지 않는다.
- 최상위 `purpose` 하나를 사용하며 개별 command에는 `purpose`를 넣지 않는다.
- 각 command는 기존 `read`/`search`/`write`/`manage`와 같은 validation, permission, service transaction을 사용한다.
- 각 command의 필수 필드는 해당 tool의 필수 필드를 따른다.
- 각 command는 해당 `tool` branch의 스키마를 사용한다. 런타임 preflight는 여러 오류를 한 번에 수집하기 위해 raw command를 받은 뒤 같은 직접 tool 입력으로 변환한다.
- 독립적인 `read`/`search`는 최대 4개까지 병렬 실행한다. 앞선 mutation과 target 범위가 겹치지 않는 뒤쪽 `read`/`search`도 병렬화하며, exact path/subtree/Space 범위가 겹치면 순서를 보존한다.
- 검증된 command는 접근 범위와 실행 등급으로 분류한 뒤 명시적인 의존성 그래프를 만든다. 그래프 간선은 정합성 순서만 나타내며 검색 동시 실행 제한은 별도로 적용한다.
- `mv`, `cp`, `rm`, `mkdir(parents=true)`는 전체 하위 구조에 미치는 범위를 실행 전에 확정할 수 없으므로 structural barrier다. 모든 앞선 command가 끝난 뒤 단독 실행하며 모든 뒤 command는 barrier 완료를 기다린다.
- `write`/`manage` mutation끼리는 fail-fast 순서를 보존하기 위해 순차 실행한다.
- 결과는 실제 완료 시점과 관계없이 입력 index 순서로 반환한다.
- 응답은 입력 순서상 첫 실패를 보고하고 이후 의존 command는 실행하지 않는다. 이미 시작된 독립 `read`/`search`는 완료될 수 있으나 실패 뒤 결과에는 포함하지 않는다.
- 이미 성공한 command는 rollback하지 않는다.
- `run_sequence` 안에서 `run_sequence`를 다시 호출할 수 없다.
- 결과는 성공한 command의 결과와 실패 위치를 반환한다.

```json
{
  "ok": false,
  "phase": "runtime",
  "executed": true,
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
  },
  "next_action": null
}
```

Preflight와 runtime 실패는 `ok`, `phase`, `executed`, `completed`, `failed_index`, `results`, `next_action`을 공통 상태 필드로 사용한다. MCP transport는 유지하므로 preflight 실패는 JSON-RPC error `data`에, 실행을 시작한 뒤의 실패는 정상 tool result의 `ok=false` payload에 담긴다. Runtime의 최상위 `next_action`과 `error.data.next_action` 필드 경로는 모두 실패한 `commands[index]` 기준으로 보정된다. 성공 payload는 `phase=complete`, `executed=true`를 사용한다.
