# UI 데이터와 흐름

## Backend 자원

| 자원 | UI 위치 | 주요 API |
|---|---|---|
| me/session | Auth, Settings | `GET /api/v1/me` |
| spaces | Space Library, ActivityRail | `GET/POST/PATCH/DELETE /api/v1/spaces` |
| space usage | Space Library cards/Inspector | `GET /api/v1/me/usage`, `POST /api/v1/spaces/{space_id}/actions/reconcile-usage` |
| nodes | Files, Recent, Editor, Inspector | `/api/v1/spaces/{space_id}/nodes...` |
| text | EditorArea | `/api/v1/spaces/{space_id}/text/{node_id}` |
| files | EditorArea | `/api/v1/spaces/{space_id}/files/{node_id}` |
| agents | Settings Agents | `/api/v1/agents` |
| agent API keys | Settings Agents | `/api/v1/agents/{id}/keys` |
| connections | Settings Agents | `/api/v1/spaces/{space_id}/agents` |

## 상태 분류

| 상태 | 소유자 | 저장 |
|---|---|---|
| 서버 자원 | React Query | Cache only |
| active space id | UI store | Local storage |
| editor groups, active group, mode, navigation history | UI store | Space별 local storage snapshot |
| opened node snapshot | UI store | Space별 local storage snapshot |
| primary/aux sidebar visibility | UI store | Local storage |
| primary sidebar width | UI store | Session only |
| Files/Recent ratio, section open, density | UI store | Session only |
| expanded folders | UI/component state | Session only |
| theme | UI store | Local storage |
| text draft | draft/component state | Session only |
| hover/menu/drag/scroll | component state | 저장 안 함 |

- Server collection과 Text/File content를 UI store에 복제하지 않는다.
- EditorGroup은 현재 Node snapshot과 pane별 navigation history를 보관한다. History는 ID·당시 이름·kind만, 현재 Node 포함 최대 50개다.
- Workbench snapshot은 browser-local best-effort이며 계정/서버 정본이나 다른 브라우저와의 동기화 대상이 아니다.
- 최근 20개 Space snapshot만 유지한다. 손상되거나 현재 Space와 맞지 않으면 폐기한다.
- Space 전환 시 이전 snapshot을 저장하고 선택 Space의 snapshot을 복원한다. 없으면 빈 group으로 시작한다.
- Saved workspace reset은 저장된 pane snapshot과 panel visibility만 지운다. Note/File/Space와 서버 자원은 삭제하지 않는다.
- Cursor·scroll position은 reload 후 복원하지 않는다.

## Auth

| 사건 | 처리 |
|---|---|
| App load | `GET /api/v1/me` 성공 시 AppShell |
| V1의 401 | Session reset → AuthScreen |
| Logout | `POST /auth/logout` → session reset → AuthScreen |
| `/me` 503 `auth_unavailable` | Session 유지, 일시 장애/재시도 표시 |

Session refresh는 BE가 처리한다. FE는 refresh token을 저장하거나 refresh endpoint를 직접 호출하지 않는다.

## Space

### ActivityRail

Space initials, selected state, add-space와 settings를 표시한다. 정렬은 `sort_order`, account/settings는 SettingsModal에 둔다.

### Select

이전 snapshot 저장 → `activeSpaceId` 변경 → 선택 snapshot/빈 group 복원 → `lastActiveSpaceId` 저장 → mobile sheets 닫기.

### Create

SpaceAddButton → dialog → `POST /api/v1/spaces` → 목록 refresh → 새 Space 선택.

### Reorder

Drag/drop indicator → `sort_order` 계산 → `POST /api/v1/spaces:reorder`로 일괄 저장 → 목록 refresh.

### Delete

Confirm → `DELETE /api/v1/spaces/{space_id}` → 목록 refresh → 관련 editor groups 제거.

## PrimarySidebar

### FilesSection

```text
GET /api/v1/spaces/{space_id}/nodes/{folder_id}/children?view=summary&limit=100&cursor=...
POST /api/v1/spaces/{space_id}/nodes:batchListChildren
GET /api/v1/spaces/{space_id}/nodes/{node_id}/reveal
```

- Root `/`는 숨긴다. Folder click은 expand/collapse만, Text/File click은 active EditorGroup 열기다.
- Drag/drop은 folder 안으로 이동하며 sibling manual reorder는 없다.
- Root/empty/folder context에 create/upload를 제공한다. Writable empty editor에는 root 대상 `Record audio`도 제공한다.

### Files load more

| 상황 | 조회·cache |
|---|---|
| Cold tree 복원 | Cache 없는 root/expanded folder의 첫 page를 최대 16 parents씩 batch; folder별 children cache에 채움 |
| Batch 실패 | Folder별 query로 복구 |
| Folder 펼치기 | 첫 page 조회 |
| Visible sentinel이 viewport 근처 도달 | 해당 folder cursor의 다음 page를 append |
| 구조 변경 | Continuation을 버리고 첫 page만 재조회; 이전 cursor로 모든 page refetch 안 함 |

Pagination은 folder별로 독립적이며 root/expanded folder 모두 같은 children API cursor를 사용한다. Batch는 cold 복원에만 쓴다.

### RecentSection

```text
GET /api/v1/spaces/{space_id}/nodes?view=summary&sort=updated_at_desc&limit=50&cursor=...
```

Recent는 항상 PrimarySidebar에 표시한다. Generic node-list와 visible sentinel로 page를 이어 읽고 invalidation 시 continuation을 버려 첫 page만 다시 읽는다.

선택 → Files reveal → 응답 target을 canonical node query에 저장 → editor 열기. Reveal 실패 시에만 canonical detail 조회로 fallback하며 실패 자체가 open을 막지는 않는다.

## Node actions

### Create

Parent folder 선택 → folder/text `POST` → 해당 children/Recent refresh → 필요 시 생성 Node 열기.

### Upload file

```text
파일 선택·이름 확인 -> POST /file-uploads
-> single PUT 또는 multipart part URLs/PUT
-> POST /file-uploads/{upload_id}/complete (multipart ETags)
-> 완성 Node cache + 대상 children/Recent refresh
```

| 정책 | 동작 |
|---|---|
| Queue | 앱 memory에 최대 2 files; Space/Node 이동 중 계속 실행. Multipart는 file당 4 parts 병렬 |
| Multipart | 100 MiB 초과는 64 MiB parts, URL 16개씩; 실패 part만 새 URL로 최대 3회 전송 |
| 취소/최종 실패 | Backend 정리 요청; 요청 실패 시 inactivity cleanup |
| 새로고침/tab 종료 | 이어서 전송하지 않음; 미완료 object는 backend 정리 |
| UploadProgressDock | 진행/실패 항목 표시, 시작 시 대상 Space/folder path snapshot |
| 실패 항목 | 처음부터 재시도 또는 제거 |
| 완료 항목 | 잠시 표시 후 제거, Changes event가 기록 정본. 현재 editor를 File로 이동하지 않음 |

### Play audio

Backend가 검증한 audio/container만 `GET /api/v1/spaces/{space_id}/files/{node_id}/audio-preview-url`의 짧은 inline URL로 native player에서 stream한다. Declared media type만 신뢰하지 않는다.

Player는 `preload="metadata"`와 native 재생/일시정지/seek를 사용하고 전체 File을 Blob으로 복사하지 않는다. URL 만료 후 실패는 새 URL을 한 번 받아 복구한다. Download는 항상 fallback이며 client-side encrypted/미검증 File은 Download만 제공한다.

### Preview DOCX

검증된 DOCX → `GET /api/v1/spaces/{space_id}/files/{node_id}/docx-preview-url` → scriptless sandbox에서 editor 폭에 맞춰 연속 렌더링.

Page width/height/break를 강제하지 않는다. 넓은 embedded content만 문서 안에서 가로 탐색한다. 표·이미지·문단 서식은 유지하며 정확한 인쇄 배치는 Download 원본이 정본이다.

### Record audio

```text
Create > Record audio -> runtime 지원 확인 -> same-origin lock -> microphone 권한
-> WebM/Opus 녹음 (5초 chunks) -> Pause/Resume -> Stop & save
-> 활성 Space root의 YYYY-MM-DD-HHmmss-record.webm 생성 -> 기존 upload queue
```

| 항목 | 규칙 |
|---|---|
| 지원 확인 | Secure context, `getUserMedia`, `MediaRecorder.isTypeSupported("audio/webm;codecs=opus")`, `navigator.locks`, `navigator.wakeLock`을 runtime 검사. Browser 이름/버전 추정이나 조용한 codec 변경 금지. 필수 secure context/capture/lock 또는 WebM/Opus 미지원이면 녹음 차단 |
| Recording lock | Same-origin `notegate:audio-recording`을 대기 없이 획득. 실패하면 다른 tab 녹음 안내만 표시하고 microphone 권한 요청 안 함 |
| Capture/encode | `ideal` 48 kHz mono, echo cancellation/noise suppression/AGC off; WebM/Opus 64 kbps |
| Metadata | `notegate-meeting-llm-v1` profile에 요청·실제 sampleRate/sampleSize/channelCount, echo/noise/AGC, MIME/bitrate 저장. Device ID/group ID/label 저장 금지 |
| 보존본 | 최초 WebM/Opus이며 raw/lossless는 아님. LLM별 downmix/resample은 별도 파생본 |
| Pause/Resume | 같은 Blob의 data 수집 중단/재개와 활성 timeline segment 닫기/새 segment 열기, File 분리 안 함. Pause도 Stop/Discard 가능하며 mic·recording lock·Wake Lock 유지 |
| Timeline clock | Duration/offset은 `performance.now()` monotonic ms, session 시작/종료만 ISO 8601 wall clock. Recorded duration은 pause 제외, wall duration은 포함 |
| Timeline metadata | Top-level `recording_timeline` object와 `recording_segments` array. Segment는 `wall_start_offset_ms`, `wall_end_offset_ms`, `media_start_offset_ms`, `media_end_offset_ms`; wall gap이 pause |
| Segment 상한 | 16 KiB metadata 상한 때문에 최대 64개. 초과 시 최초 32 + 최근 32개, `segment_count`, `segments_included_count`, `segments_omitted_count` 기록 |
| RecordingDock | Desktop/tablet bottom-right의 UploadProgressDock 위, mobile full-width bottom stack. 접어도 Recording/elapsed header 유지. Mic level 최대 15 fps, 상태의 유일한 신호로 사용 안 함 |
| 녹음 중 허용 | 현재 Space Files 탐색, 문서 열기/스크롤, Outline, 검색, 복사 |
| 녹음 중 차단 | Create/edit/move/delete, Space/Settings 전환 |
| Stop & save | Queue 등록 즉시 dock 닫고 일반 작업 복귀, mic/recording lock 해제. 공통 2 files queue로 전송 중 다음 녹음 가능 |
| Wake Lock | 녹음 시작부터 upload 완료/실패까지 요청 후 해제. OS 거부/해제는 녹음을 막지 않으며 visible 복귀 시 진행 중 녹음/upload에 재요청 |
| Background | Upload 완료 전 foreground 유지 필요. Backend가 완료 확인한 뒤 후처리는 화면 상태와 독립 |
| 복구 | Chunks는 memory뿐. 저장 전 reload/tab 종료/browser·OS 강제 종료는 복구 불가 |

표준: [MediaStream Recording](https://www.w3.org/TR/mediastream-recording/), [Web Locks](https://www.w3.org/TR/web-locks/), [Screen Wake Lock](https://www.w3.org/TR/screen-wake-lock/). iOS/iPadOS Home Screen Web App Wake Lock은 [Safari 18.4](https://webkit.org/blog/16574/webkit-features-in-safari-18-4/)부터 지원한다.

### Download file

Browser 기본 다운로드 관리자를 사용한다.

### Rename

`PATCH /nodes/{node_id}` → node/children/Recent refresh → 열린 Node snapshot 갱신.

### Move

`POST /nodes/{node_id}/move` → 이전/새 parent, reveal, Recent refresh → 열린 Node snapshot 갱신.

### Delete

Confirm → `DELETE /nodes/{node_id}` → children/Recent refresh → 열린 groups/history의 삭제 Node 제거.

## EditorArea

| Kind | 조회 |
|---|---|
| Folder | Node detail |
| Text | Detail + Text content |
| File | Detail + metadata/download |

Header 왼쪽에 이름·pane별 Back/Forward와 `<space>:/path` 복사 아이콘을 표시한다. Path/metrics는 Inspector에 둔다.

Text는 preview가 기본이며 plain text는 메모처럼 표시한다. Markdown은 GFM/code highlight/Mermaid, leading YAML object는 Obsidian-style Properties로 표시하고 raw YAML prose는 렌더링하지 않는다. Frontmatter는 content이며 Inspector metadata와 동기화하지 않는다. JSON/JSONL/YAML/TOML은 기본 expanded Tree/Source, edit mode는 line number를 제공한다.

### Open

현재 reference를 active group back history에 추가 → forward 비우기 → 열린 snapshot 설정 → kind별 detail/content 조회 → active Node Inspector 표시. 같은 Node를 다시 열면 history에 추가하지 않는다.

### Back/Forward

| 단계/결과 | 동작 |
|---|---|
| 최근 reference 선택 | Target/ancestor reveal |
| Reveal 성공 | Target cache, 현재 reference를 반대 history로 이동 후 열기 |
| Reveal 실패 | Canonical detail 조회로 fallback |
| Reveal/detail 404 | Missing reference 버리고 같은 방향의 다음 항목 탐색 |
| 그 외 detail 실패 | 현재 Node와 두 histories 유지, toast |
| 요청 중 group/Space 변경 | 늦은 응답 무시 |

History는 group별 독립적이다. 새 Node를 열면 forward를 비우고 새 group에는 현재/선택 Node만 넣으며 history를 복사하지 않는다. Space 전환/reload는 snapshot에서 복원한다. Rename/move는 이름 snapshot을 갱신하고 delete는 reference를 제거한다.

### Markdown image preview

Near-viewport path 요청을 같은 microtask에서 병합 → `POST /file-previews:batchResolve` → 순서대로 path별 결과와 ready Node URL cache → 이미지별 missing/unsupported/transient 실패를 독립 처리.

단일 File rename/move는 이전 path cache만 제거한다. Folder 변경/외부 path event는 active Space preview cache를 제거한다. Presigned URL 만료는 해당 path만 재조회한다. Batch 상한은 [구현 규칙](03-implementation.md#external-sync)을 따른다.

### Split

최대 3 groups. 오른쪽에 현재 active Node/빈 상태와 빈 history의 group을 추가한다.

### Save text

`PUT /text/{node_id}` + `expected_sha256` → 성공 시 preview 전환·Node cache 갱신·Text/Recent refresh. Conflict는 충돌 상태로 표시한다.

### External sync

Visible tab은 active Space event를 마지막 적용 ID 이후부터 모든 page 오름차순으로 읽어 Node/content·해당 parent children·Recent를 invalidate한다. Expired/unknown token은 file-related cache family를 한 번 refresh하고 새 token을 설정한다. 열린 Node 404는 group을 비운다. Interval/token 적용 규칙은 [External sync 구현](03-implementation.md#external-sync)이 정본이다.

## Structured preview

Tree/Source toggle은 preview mode만 바꾼다. Expand/Collapse all은 Tree mode에서만 적용한다.

## Inspector

| 표시 | 내용 |
|---|---|
| 기본 | Name, path, kind, folder child count/File size/Text line count, metadata JSON |
| 설정 | 현재 Node 검색 포함 여부, Text 서버 암호화 상태 |
| 접힌 System details | Created/updated attribution, internal ID |

선택 없어도 빈 Inspector를 표시한다. 검색 포함은 `PUT /nodes/{node_id}/external-access-policy`, Text 암호화는 `PUT /text/{node_id}/encryption`으로 독립 변경한다. Space 기본값은 새 Node에만, Inspector 변경은 현재 Node에 즉시 적용한다. Metadata는 암호화된 content가 아니며 읽기 전용이다.

## Settings

| Tab | 내용 |
|---|---|
| General | Saved workspace reset, About의 루트 `VERSION`과 공식 GitHub 링크 |
| Account | User/account, theme, User MCP OAuth 2.1 URL, sign out |
| Agents | 공용 Agent MCP URL, REST API `/api/v2` base URL/문서, agent list |

Agent 관리 권한이 있는 caller에게만 Agents를 표시한다. 공용 URL은 상단 한 번만 표시한다. 한 번에 한 Agent만 펼치고 그 아래에 Space permission/API keys만 둔다. `scopes`는 표시하지 않는다. 제품 표시는 `REST API`, 문서는 새 tab으로 연다.

## Context menus

우클릭은 shortcut이며 같은 action에 버튼/overflow/dialog/touch 대안을 제공한다. Text editor native menu는 유지한다. Destructive action은 confirm, touch는 long-press/visible overflow를 사용한다.

| Surface | Target | Actions |
|---|---|---|
| ActivityRail | Space | Select, rename, delete, copy id |
| Files | Empty/root | New folder, new document, upload file |
| Files | Folder | Open/toggle, create child, upload, rename, move, copy path, delete |
| Files | Text | Open, open in new group, rename, move, copy path, delete |
| Files | File | Open, open in new group, download, rename, move, copy path, delete |
| EditorHeader | Node | Rename, move, delete, download if File |
| Inspector | Metadata | View system metadata |
