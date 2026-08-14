# UI 데이터와 흐름

## Backend 자원

| 자원 | UI 위치 | 주요 API |
|---|---|---|
| me/session | Auth, Settings | `GET /api/v1/me` |
| spaces | Space Library, ActivityRail | `GET/POST/PATCH/DELETE /api/v1/spaces` |
| space usage | Space Library cards/Inspector | `GET /api/v1/me/usage`, `POST /api/v1/spaces/{space_id}/usage/reconcile` |
| nodes | Files, Recent, Editor, Inspector | `/api/v1/spaces/{space_id}/nodes...` |
| text | EditorArea | `/api/v1/spaces/{space_id}/text/{node_id}` |
| files | EditorArea | `/api/v1/spaces/{space_id}/files/{node_id}` |
| agents | Settings Agents | `/api/v1/agents` |
| agent API keys | Settings Agents | `/api/v1/agents/{id}/keys` |
| connections | Settings Agents | `/api/v1/spaces/{space_id}/agents` |

## 상태 분류

| 상태 | 소유자 | 저장 |
|---|---|---|
| 서버 자원 | React Query | cache only |
| active space id | UI store | local storage |
| editor groups, active group, mode, navigation history | UI store | space별 local storage snapshot |
| opened node snapshot | UI store | space별 local storage snapshot |
| primary/aux sidebar visibility | UI store | local storage |
| primary sidebar width | UI store | session only |
| Files/Recent ratio, section open, density | UI store | session only |
| expanded folders | UI/component state | session only |
| theme | UI store | local storage |
| text draft | draft/component state | session only |
| hover/menu/drag/scroll | component state | 저장 안 함 |

규칙:

- 서버 collection은 UI store에 복제하지 않는다.
- EditorGroup은 현재 열린 node snapshot과 해당 pane의 최근 navigation history를 보관한다.
- navigation history는 node ID, 당시 이름, kind만 저장하며 현재 node를 포함해 최대 50개다.
- text body와 file content는 UI store에 보관하지 않는다.
- space별 workbench snapshot은 browser-local best-effort 상태다. 계정/서버 정본이 아니며 다른 브라우저로 동기화하지 않는다.
- workbench snapshot은 최근 20개 space까지만 유지한다. 손상됐거나 현재 space와 맞지 않는 snapshot은 폐기한다.
- space 전환 시 현재 space snapshot을 저장하고, 선택한 space snapshot이 있으면 복원한다. 없으면 빈 editor group으로 시작한다.
- Settings의 Saved workspace reset은 browser에 저장된 pane snapshot과 panel visibility만 지운다. note, file, space, 서버 자원은 삭제하지 않는다.
- cursor와 scroll position은 reload 후 복원하지 않는다.

## Auth

```text
App load
-> GET /api/v1/me
-> success: AppShell
-> 401: AuthScreen
```

```text
Logout
-> POST /auth/logout
-> reset session
-> AuthScreen
```

```text
any /api/v1/* returns 401
-> reset session
-> AuthScreen
```

Browser session refresh는 server-side flow다. FE는 refresh token을 저장하거나 직접 refresh endpoint를 호출하지 않는다. `/api/v1/me` 401은 재로그인 필요 상태로 처리하고, 503 `auth_unavailable`은 세션을 지우지 않는 일시 장애/재시도 상태로 처리한다.

## Space

### ActivityRail

표시:

- space initials.
- selected state.
- add-space button.
- settings button.

규칙:

- space 정렬은 `sort_order` 기준.
- drag reorder는 `POST /api/v1/spaces:reorder`로 일괄 저장한다.
- account/settings는 SettingsModal에 둔다.

### Select

```text
click space
-> persist previous space workbench snapshot
-> set activeSpaceId
-> restore selected space workbench snapshot or empty editor groups
-> persist lastActiveSpaceId
-> close mobile sheets
```

### Create

```text
SpaceAddButton
-> dialog
-> POST /api/v1/spaces
-> refresh spaces
-> select created space
```

### Reorder

```text
drag space
-> show drop indicator
-> compute sort_order
-> POST /api/v1/spaces:reorder
-> refresh spaces
```

### Delete

```text
explicit delete
-> confirm
-> DELETE /api/v1/spaces/{space_id}
-> refresh spaces
-> clear related editor groups
```

## PrimarySidebar

### FilesSection

데이터:

```text
GET /api/v1/spaces/{space_id}/nodes/{folder_id}/children?view=summary&limit=100&cursor=...
POST /api/v1/spaces/{space_id}/nodes:batchListChildren
GET /api/v1/spaces/{space_id}/nodes/{node_id}/reveal
```

규칙:

- root `/`는 보이지 않는다.
- folder row click은 editor node를 열지 않고 expand/collapse만 수행한다.
- text/file row click은 active EditorGroup에 연다.
- drag/drop은 node를 folder 안으로 이동한다.
- sibling manual reorder는 하지 않는다.
- root/empty/folder context에서 create/upload를 제공한다. writable empty editor는 root 대상 `Record audio`도 함께 제공한다.

### Files load more

```text
restore root and multiple expanded folders with missing cache
-> fetch their first pages in batches of at most 16 parents
-> seed each folder's existing children query cache

expand root/folder
-> fetch first children page for that folder
-> scroll near folder page end
-> fetch next cursor page for that folder
-> append visible child rows
```

규칙:

- children pagination은 folder별로 독립적이다.
- cold tree 복원만 first-page batch API를 사용한다. Batch 실패 시 기존 folder별 query로 복구한다.
- root와 각 expanded folder는 같은 children API cursor를 사용한다.
- 자동 load-more는 visible sentinel이 viewport 근처에 들어올 때 수행한다.
- 구조 변경 후 이미 여러 page가 열린 folder는 기존 continuation page를
  버리고 첫 page만 다시 읽는다. 이전 cursor로 모든 page를 순차 refetch하지 않는다.

### RecentSection

데이터:

```text
GET /api/v1/spaces/{space_id}/nodes?view=summary&sort=updated_at_desc&limit=50&cursor=...
```

규칙:

- Recent는 항상 PrimarySidebar에 있다.
- generic node-list API를 사용한다.
- visible load-more sentinel을 통해 cursor page를 이어서 표시한다.
- invalidation 시 기존 continuation page를 버리고 첫 page만 다시 읽는다.
- row 선택 시 node를 열고 Files reveal을 시도한다.
- reveal 응답의 target을 canonical node query에 채운 뒤 editor를 연다.
- reveal 실패는 open을 막지 않으며, 이때만 canonical node detail 조회로
  fallback한다.

## Node actions

### Create

```text
folder/text create
-> choose parent folder
-> POST node/text API
-> refresh affected children/recent
-> open created node when applicable
```

### Upload file

```text
select file
-> confirm node name
-> POST /file-uploads
-> single: PUT all bytes to the presigned URL
-> multipart: request part URLs, PUT at most 4 parts concurrently
-> POST /file-uploads/{upload_id}/complete with multipart ETags
-> cache completed node + refresh destination children/recent
```

규칙:

- upload는 앱 범위의 memory queue에서 최대 2개 파일까지 실행하므로 space나 node를 이동해도 계속된다. Multipart는 파일당 최대 4개 part를 병렬 전송한다.
- 새로고침이나 tab 종료 뒤에는 이어서 전송하지 않는다. 완료되지 않은 object 정리는 backend 정책을 따른다.
- 100MiB 초과 파일은 64MiB part로 나누고 URL은 16개씩 발급받는다. 실패한 part만 새 URL로 최대 3회 전송한다.
- 취소하거나 최종 실패하면 backend에 upload 정리를 요청한다. 요청 실패 시 backend의 inactivity cleanup이 처리한다.
- 진행 중이거나 실패한 항목은 전역 UploadProgressDock에서 확인한다. 시작 시 대상 space와 folder path를 snapshot으로 보관한다.
- 실패한 항목은 처음부터 재시도하거나 목록에서 제거할 수 있다.
- 완료 항목은 잠시 표시한 뒤 제거한다. 완료 기록의 정본은 Changes event다.
- 완료 시 현재 editor를 file node로 이동하지 않는다.

### Play audio

```text
open a verified audio File
-> GET /api/v1/spaces/{space_id}/files/{node_id}/audio-preview-url
-> receive a short-lived inline object URL with a server-selected audio media type
-> stream with the native browser player
-> retain Download as the fallback action
```

규칙:

- declared media type만 신뢰하지 않고 backend가 확인한 audio/container 조합에만 URL을 발급한다.
- browser player는 `preload="metadata"`를 사용하며 전체 File을 application memory의 Blob으로 복사하지 않는다.
- 재생, 일시정지, seek를 browser native control로 제공한다. URL 만료 뒤 media request가 실패하면 새 URL을 한 번 발급받아 복구한다.
- client-side encrypted File과 확인되지 않은 media type은 inline 재생하지 않고 Download만 제공한다.

### Record audio

```text
Create > Record audio
-> verify secure context + getUserMedia + WebM/Opus MediaRecorder + browser lock manager
-> acquire the same-origin notegate:audio-recording lock without waiting
   -> unavailable: report another NoteGate tab is recording and do not request microphone permission
-> request microphone permission
-> request 48 kHz mono capture with echo cancellation, noise suppression, and AGC disabled
-> record WebM/Opus at 64 kbps
-> target the active Space root with YYYY-MM-DD-HHmmss-record.webm
-> record 5-second chunks in memory
-> Pause: stop gathering bytes into the current Blob and close the active timeline segment
-> Resume: continue gathering into the same Blob and open the next timeline segment
-> Stop & save
-> create one File with requested/actual capture settings, timeline summary, ordered recording segments, and use the existing upload queue
-> release Screen Wake Lock after upload completes or fails
```

규칙:

- browser 이름/버전을 추정하지 않고 `getUserMedia`, `MediaRecorder.isTypeSupported("audio/webm;codecs=opus")`, `navigator.locks`, `navigator.wakeLock`을 runtime에 검사한다. 고정 format을 지원하지 않으면 다른 codec으로 조용히 변경하지 않고 녹음을 막는다.
- 48 kHz와 mono, 음성 가공 비활성화는 `ideal` capture constraint다. 장치/OS/browser가 선택한 실제 `sampleRate`, `sampleSize`, `channelCount`, echo cancellation, noise suppression, AGC와 recorder MIME/bitrate는 `notegate-meeting-llm-v1` profile metadata로 File Node 생성 시 함께 저장한다. Privacy/fingerprinting surface인 device ID, group ID, device label은 저장하지 않는다.
- WebM/Opus File은 최초 보존본이지만 raw/lossless audio는 아니다. LLM별 downmix/resample은 후처리에서 파생본으로 만들고 보존본을 대체하지 않는다.
- pause/resume은 File을 나누지 않는다. `MediaRecorder.pause()`는 현재 Blob을 유지한 채 data 수집을 멈추고 `resume()`은 같은 Blob에 이어서 수집한다.
- timeline duration과 segment offset은 `performance.now()` 기반 monotonic milliseconds로 계산하고, session 시작/종료만 ISO 8601 wall-clock timestamp로 저장한다. recorded duration은 pause를 제외하며 wall duration은 pause를 포함한다.
- metadata는 depth 제한을 지키기 위해 summary를 top-level `recording_timeline` object로, interval 목록을 top-level `recording_segments` array로 둔다. 각 segment는 `wall_start_offset_ms`, `wall_end_offset_ms`, `media_start_offset_ms`, `media_end_offset_ms`를 가진다. pause 구간은 인접 segment 사이의 wall-time gap으로 계산한다.
- `recording_segments`는 16 KiB 상한을 위해 최대 64개를 저장한다. 이를 넘으면 최초 32개와 최근 32개를 보존하고 `segment_count`, `segments_included_count`, `segments_omitted_count`로 생략 여부를 명시한다.
- paused 상태에서도 `Stop & save`와 `Discard`를 허용하고 Screen Wake Lock, microphone stream, same-origin recording lock은 유지한다.
- 녹음 중에는 `RecordingDock`을 desktop/tablet의 bottom-right transfer stack에서 `UploadProgressDock` 위에 표시한다. panel은 접을 수 있지만 `Recording`과 elapsed time은 header에 남는다. mobile에서는 full-width bottom stack을 사용한다. 실제 microphone level은 최대 15 fps로 표시하며 녹음 상태의 유일한 신호로 사용하지 않는다.
- 녹음 중에는 현재 Space의 Files 탐색, 문서 열기/스크롤, Outline, 검색, 복사를 유지하고 create/edit/move/delete와 Space/Settings 전환을 막는다.
- `Stop & save`가 File을 upload queue에 넣으면 `RecordingDock`을 즉시 닫고 일반 작업 상태로 돌아간다. 다른 upload와 녹음 File은 같은 최대 2개 병렬 queue를 사용하므로 이전 녹음이 전송되는 동안 다음 녹음을 시작할 수 있다.
- 녹음 시작부터 NoteGate upload 종료까지 Screen Wake Lock을 요청한다. Wake Lock은 보조 기능이므로 OS가 거부하거나 해제해도 녹음 자체는 계속된다.
- 문서가 다시 visible 상태가 되면 진행 중인 녹음/upload의 Wake Lock을 다시 요청한다.
- 브라우저 background upload는 보장하지 않으므로 upload 완료 전에는 NoteGate를 foreground에 유지해야 한다. 서버가 upload 완료를 확인한 뒤의 후처리는 화면 상태와 무관하다.
- 녹음 chunk는 memory에 보관한다. tab 새로고침, 종료, browser/OS 강제 종료 시 저장 전 녹음은 복구하지 않는다.
- 표준 근거는 [MediaStream Recording](https://www.w3.org/TR/mediastream-recording/), [Web Locks](https://www.w3.org/TR/web-locks/), [Screen Wake Lock](https://www.w3.org/TR/screen-wake-lock/)이다. iOS/iPadOS Home Screen Web App의 Screen Wake Lock은 [Safari 18.4](https://webkit.org/blog/16574/webkit-features-in-safari-18-4/)부터 지원된다.

### Download file

- 파일 다운로드는 브라우저 기본 다운로드 관리자를 사용한다.

### Rename

```text
rename
-> PATCH /nodes/{node_id}
-> refresh node, children, recent
-> update opened node snapshot
```

### Move

```text
move into folder
-> POST /nodes/{node_id}/move
-> refresh old/new parents, reveal, recent
-> update opened node snapshot
```

### Delete

```text
delete
-> confirm
-> DELETE /nodes/{node_id}
-> refresh children/recent
-> clear deleted node from opened editor groups and navigation history
```

## EditorArea

node kind별 데이터:

```text
folder -> node detail
text   -> node detail + text content
file   -> node detail + file metadata/download
```

규칙:

- header 왼쪽에는 node name과 pane별 Back/Forward를 표시한다.
- header의 node name 옆에는 내부 `<space>:/path`를 복사하는 `Copy path` 아이콘을 둔다.
- path와 metrics는 Inspector에 둔다.
- text preview가 기본이다.
- plain text는 단순 메모처럼 보여준다.
- markdown은 GFM, code highlight, Mermaid를 지원한다.
- markdown preview는 leading YAML frontmatter object를 Obsidian-style Properties로 표시하고 raw YAML block은 본문 prose로 렌더링하지 않는다.
- markdown frontmatter는 Text content이며 Inspector metadata와 동기화하지 않는다.
- JSON/JSONL/YAML/TOML은 Tree/Source view를 제공한다.
- structured tree는 기본 expanded 상태다.
- edit mode는 line number를 보여준다.

### Open

```text
open node
-> push current node reference to the active EditorGroup back history
-> clear forward history
-> set active EditorGroup node snapshot
-> fetch detail/content by kind
-> show Inspector for active node
```

같은 node를 다시 열면 history에 중복 추가하지 않는다.

### Back/Forward

```text
click Back/Forward
-> read the nearest node reference from that EditorGroup
-> reveal target and ancestors
-> success: cache target, move current node reference to the opposite history, open target
-> reveal failure: GET canonical node detail as fallback
-> reveal/detail 404: discard missing reference and continue in the same direction
-> other detail failure: keep current node and both histories, then show toast
```

규칙:

- history는 EditorGroup별로 독립적이다.
- 새 node를 연 뒤에는 forward history를 비운다.
- 새 group은 현재 node 또는 선택한 node만 가지며 기존 group history를 복사하지 않는다.
- space 전환과 reload 후에도 space별 workbench snapshot에서 복원한다.
- node rename/move는 저장된 이름 snapshot을 갱신하고 delete는 해당 reference를 제거한다.
- 요청 중 group이나 space가 바뀌면 늦게 도착한 응답을 적용하지 않는다.

### Markdown image preview

```text
near-viewport image paths
-> same microtask requests coalesce
-> POST /file-previews:batchResolve
-> cache each ordered path result and each ready node preview URL
-> render ready results; isolate missing, unsupported, and transient failures per image
```

로컬 단일 file rename/move는 이전 path cache만 제거한다. Folder 변경과 외부 path change event는 영향받은 하위 path를 직접 알 수 없으므로 active Space의 Markdown image preview cache를 제거한다. 만료된 presigned URL은 해당 path만 다시 배치 조회한다.

### Split

```text
split
-> if group count < 3: add group to the right
-> new group starts with current active node or empty state and empty navigation history
```

### Save text

```text
edit text
-> PUT /text/{node_id} with expected_sha256
-> success: preview mode + patch cached node representations + refresh text/recent
-> conflict: show conflict state
```

### External sync

```text
visible tab: poll active-space changes after the last applied event id
-> drain every page in ascending event order
-> invalidate changed node/content + affected parent children + Recent
-> expired/unknown token: refresh file-related cache families once and establish a new token
-> opened node 404: clear editor group
```

## Structured preview

```text
Tree/Source toggle
-> change preview mode only
```

```text
Expand all / Collapse all
-> applies only in Tree mode
```

## Inspector

표시:

- name, path, kind.
- folder child count 또는 file size와 Text line count.
- metadata JSON.
- 현재 node의 검색 포함 여부.
- Text의 현재 서버 관리 암호화 상태.
- 접힌 System details 안의 created/updated attribution과 internal id.

규칙:

- 선택 node가 없어도 빈 Inspector를 렌더링한다.
- 검색과 Text 암호화 설정은 서로 독립적으로 변경한다.
- 검색 포함 여부는 `PUT /nodes/{node_id}/search-policy`로 변경한다.
- Text 암호화는 `PUT /text/{node_id}/encryption`으로 변경한다.
- Space의 기본값은 새 node 생성에만 적용하고 Inspector는 선택한 node의 현재 상태를 즉시 변경한다.
- metadata는 encrypted content가 아니며 읽기 전용으로 표시한다.

## Settings

Tabs:

```text
General | Account | Agents
```

General:

- saved workspace reset.
- About에 `VERSION` 파일 기준의 현재 NoteGate 버전과 공식 GitHub 저장소 링크를 표시한다.

Account:

- current user/account.
- theme.
- user MCP OAuth 2.1 server URL.
- sign out.

Agents:

- 모든 agent가 공유하는 agent MCP server URL.
- 모든 agent가 공유하는 REST API base URL과 API 문서 링크.
- agent list.
- 한 번에 하나의 agent만 펼친다.
- 펼친 agent 안에는 space permission과 agent API keys만 둔다.

규칙:

- agent 연결 URL은 agent마다 반복하지 않고 Agents 상단 공용 영역에 한 번만 둔다.
- 제품 표시는 `REST API`로 통일하고 실제 versioned base URL은 `/api/v2`를 사용한다.
- API 문서는 새 탭으로 연다.
- agent API key는 해당 agent 아래에 둔다.
- `scopes`는 현재 정책상 표시하지 않는다.
- Agents tab은 agent 관리 권한이 있는 caller에게만 표시한다.

## Context menus

규칙:

- 우클릭은 shortcut이다.
- 같은 action은 버튼, overflow, dialog, touch fallback 중 하나로도 가능해야 한다.
- text editing 영역에서는 native context menu를 막지 않는다.
- destructive action은 confirm이 필요하다.
- touch는 long-press 또는 visible overflow를 사용한다.

| Surface | Target | Actions |
|---|---|---|
| ActivityRail | space | select, rename, delete, copy id |
| Files | empty/root | new folder, new document, upload file |
| Files | folder | open/toggle, create child, upload, rename, move, copy path, delete |
| Files | text | open, open in new group, rename, move, copy path, delete |
| Files | file | open, open in new group, download, rename, move, copy path, delete |
| EditorHeader | node | rename, move, delete, download if file |
| Inspector | metadata | view system metadata |
