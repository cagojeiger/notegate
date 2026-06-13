# UI flows

이 문서는 대시보드에서 사용자가 수행하는 핵심 흐름을 정의한다. 흐름은 현재 REST API와 `docs/spec`가 보장하는 동작만 사용한다.

## Flow boundaries

대시보드 UI 기준:

```text
Dashboard UI -> REST API -> service/domain
MCP/CLI      -> 별도 path-first 도구 표면
```

규칙:

- 대시보드는 REST API를 기준으로 화면을 그린다.
- UI는 `space_id`, `node_id`를 사용해 선택 상태를 유지한다.
- MCP/CLI의 `target` 문자열이나 sequence command를 대시보드 흐름으로 가져오지 않는다.
- 아직 backend에 없는 전역 command/search, 파일별 권한 흐름은 만들지 않는다.
- 화면 분할, sidebar 열기/닫기, pane resize는 local UI state다. Backend에 저장되는 흐름으로 정의하지 않는다.

## Workbench state rules

`Workbench`의 네 영역은 서로 직접 상태를 소유하지 않는다. 공통 상태는 성격별로 나누고, 각 region은 필요한 상태를 읽거나 action만 dispatch한다.

상태 분류:

```text
1. Server state      Backend가 정본인 resource data
2. UI state          화면 배치와 editor group 상태
3. Draft state       저장 전 편집 상태
4. Ephemeral state   hover/menu/loading 같은 일시 상태
```

선택 상태(active space/node)는 URL이나 route에 저장하지 않는다. 새로고침은 fresh로 시작하고, 마지막 active space만 local storage로 복원한다. Active node는 별도 상태로 두지 않고 active editor group의 node로 표현한다.

### 1. Server state

Backend에서 가져오는 resource state다. FE는 표시와 cache만 담당한다.

```text
Server state
├─ spaces
├─ nodes
├─ children pages
├─ text content / text metadata
├─ file metadata
├─ file content download result
└─ node metadata
```

Backend:

```text
GET /api/v1/spaces
GET /api/v1/spaces/{space_id}
GET /api/v1/spaces/{space_id}/nodes
GET /api/v1/spaces/{space_id}/nodes/{node_id}
GET /api/v1/spaces/{space_id}/nodes/{node_id}/children
GET /api/v1/spaces/{space_id}/nodes/{node_id}/reveal
GET /api/v1/spaces/{space_id}/text/{node_id}
GET /api/v1/spaces/{space_id}/files/{node_id}
GET /api/v1/spaces/{space_id}/nodes/{node_id}/metadata
```

규칙:

1. Server state는 backend가 정본이다.
2. FE store에 별도 정본으로 복사하지 않는다.
3. UI는 `space_id`, `node_id` 같은 id를 저장하고, 객체 자체는 server cache에서 읽는다.
4. Mutation 성공 후 관련 server state를 다시 불러오거나 갱신한다.
5. Text/File content body는 필요할 때만 category endpoint로 읽는다.

### 2. UI state

Backend에 저장하지 않는 화면 상태다. 브라우저 local storage로 복원할 수 있다.

```text
UI state
├─ lastActiveSpaceId   # 새로고침 시 복원하는 유일한 선택 상태
├─ activeEditorGroupId
├─ editorGroups[]
├─ primarySidebarVisible
├─ primarySidebarWidth
├─ treeRecentRatio
├─ auxiliarySidebarVisible
└─ auxiliaryActiveView
```

Region responsibilities:

1. `ActivityRail`
   - Space 목록을 보여준다.
   - Space 선택 시 active space를 바꾸고 `lastActiveSpaceId`로 local에 기억한다.
   - Node tree나 editor content를 직접 관리하지 않는다.

2. `PrimarySidebar`
   - Active space의 tree/recent 목록을 보여준다.
   - Node 선택 시 active editor group에 열고 tree selection(ephemeral)을 갱신한다.
   - Editor content를 직접 소유하지 않는다.

3. `EditorArea`
   - `editorGroups[]`와 active editor group을 보여준다.
   - Active node를 현재 editor group에 연다.
   - Split/close는 UI state만 변경한다.

4. `AuxiliarySidebar`
   - Active node의 보조 정보를 보여준다.
   - Metadata 수정처럼 backend에 저장되는 action만 REST로 보낸다.
   - Editor content나 tree selection을 직접 소유하지 않는다.

규칙:

1. UI state는 제품 데이터가 아니다.
2. Space/node 접근 권한이 사라지면 관련 UI state는 버린다.
3. Device/browser마다 달라도 되는 값만 local storage에 저장한다.
4. 새로고침은 fresh로 시작하고 `lastActiveSpaceId`만 복원한다. Editor group, tree 펼침, node 선택은 복원하지 않는다.

### 3. Draft state

저장 전 편집 상태다. Backend 저장 전까지 제품 데이터가 아니다.

```text
Draft state
├─ nodeId
├─ baseSha256
├─ content
├─ dirty
└─ lastEditedAt
```

규칙:

1. Draft는 원본 `node_id`와 `content_sha256`에 묶는다.
2. 저장 성공 후 draft를 제거한다.
3. 원본 hash가 바뀌면 자동 저장하지 않고 conflict 상태로 보여준다.
4. Encrypted Text의 plaintext draft는 기본 저장하지 않는다.
5. Encrypted Text draft 저장이 필요하면 사용자가 명시적으로 허용한 local-only 저장소에만 둔다.

### 4. Ephemeral state

짧게 유지되는 일시 상태다. 보통 component local state로 관리하고 저장하지 않는다.

```text
Ephemeral state
├─ expandedFolderIds
├─ children pagination cursor and loaded pages
├─ tree/recent scroll position
├─ hover/focus/context menu state
├─ current selection highlight
├─ upload progress
├─ inline validation message
└─ transient error toast
```

규칙:

1. Reload 후 복원하지 않아도 된다.
2. Pagination cursor는 현재 query의 continuation token이다.
3. Cursor를 장기 저장하지 않는다.
4. Right-click menu, hover, temporary drag target은 저장하지 않는다. 단, drag가 drop으로 확정된 reorder 결과는 backend `sort_order`로 저장한다.
5. Stale children page cache와 failed request body는 저장하지 않는다.

### 5. Region interaction flow

1. Space 선택

```text
ActivityRail
-> active space 변경 (lastActiveSpaceId local 기억)
-> PrimarySidebar root tree 초기화
-> EditorArea empty
-> AuxiliarySidebar empty inspector
```

1-1. Space reorder

```text
ActivityRail drag/drop
-> visible space order optimistic update
-> PATCH /api/v1/spaces/{space_id} with sort_order for changed spaces
-> GET /api/v1/spaces order remains sort_order, name, id
```

2. Node 선택

```text
PrimarySidebar
-> active EditorGroup에 node open/replace
-> tree selection(ephemeral) 갱신
-> AuxiliarySidebar inspector 갱신
```

3. Editor group 변경

```text
EditorArea
-> activeEditorGroupId 또는 editorGroups[] 변경
-> active node가 바뀌면 PrimarySidebar selection 동기화
-> AuxiliarySidebar context 동기화
```

4. Metadata 수정

```text
AuxiliarySidebar
-> REST metadata update
-> server state 갱신
-> PrimarySidebar / EditorArea / AuxiliarySidebar가 같은 node 정보를 반영
```

5. Layout action

```text
TitleBar
-> UI state 변경
-> backend resource 변경 없음
```

### 6. Server-persisted state

Backend에 저장되는 제품 데이터:

1. Space
   - name
   - sort_order

2. Node tree
   - parent
   - name
   - kind
   - sort_order

3. Node metadata
   - metadata JSON object

4. Text
   - plain content
   - encrypted payload

5. File
   - file metadata
   - file bytes

규칙:

1. REST API 호출 성공 후 모든 region은 backend 정본을 다시 반영한다.
2. Backend에는 화면 배치나 일시적 탐색 상태를 저장하지 않는다.

## Common limits the UI must respect

Backend hard limit:

```text
space count per owner user      <= 20
space live nodes                <= 25,000
space live content bytes        <= 1 GiB
path depth below root           <= 7
folder direct children          <= 1,000
children list page              default 100, max 200
text size                       <= 1 MiB
text lines                      <= 2,000
text read                       default 200 lines / 64 KiB, max 1,000 lines / 256 KiB
file upload currently stored    <= 256 KiB inline PostgreSQL
file object reserved hard cap   <= 100 MiB
node metadata JSON              <= 16 KiB
```

UI 규칙:

- Tree는 전체 space를 한 번에 로드하지 않는다.
- Folder children은 cursor 기반 page로 로드한다.
- 제한 초과는 client에서 사전 안내할 수 있지만, 최종 판정은 backend error를 따른다.
- `Text`와 `File`의 content body는 `RestNode`에 포함되지 않는다. 필요할 때 category endpoint로 별도 조회한다.

## 1. Login and enter workbench

Trigger:

```text
사용자가 AuthScreen에서 로그인한다.
```

Backend:

```text
GET /api/v1/me                         # session check
GET /auth/login?next=/                 # OAuth login start
GET /auth/callback                     # OAuth callback; sets browser session cookie
POST /auth/logout                      # clears browser session cookie
GET /api/v1/spaces?limit=...
```

UI result:

- `AuthScreen`은 먼저 `/me`로 기존 browser session을 확인한다.
- session이 없으면 OAuth login CTA를 보여준다.
- 개발/e2e 전용 user API key fallback은 보조 경로이며 기본 로그인 경로가 아니다.
- 로그인 성공 후 `AppShell`을 렌더링한다.
- `ActivityRail`에 접근 가능한 space 목록을 표시한다.
- `lastActiveSpaceId`(local)가 접근 가능하면 그 space를, 없으면 첫 space를 active로 선택하고 root children을 로드한다.
- Node 선택, editor group, tree 펼침은 복원하지 않고 fresh로 시작한다.
- Space가 없으면 empty state와 명시적 space 생성 CTA를 보여준다.

Constraints:

- 로그인 성공만으로 space를 자동 생성하지 않는다.
- OAuth session은 HttpOnly browser cookie다.
- Login `next` 값은 same-origin relative path만 사용한다.
- `/me`는 identity와 전역 capability만 반환한다. Space permission은 `/spaces` 응답에서 본다.
- Account/profile 진입은 Settings에서 처리한다.

## 2. Space create, rename, delete

Space 관리는 active node와 독립된 flow다. Space create/update/delete는 user caller만 가능하다.

### 2.1 Create space

Trigger:

```text
SpaceAddButton 또는 empty state CTA 클릭
```

UI state changes:

1. `CreateSpaceDialog`를 연다.
2. 사용자가 `name`을 입력한다.
3. 제출 중에는 dialog submit action을 loading/disabled 상태로 둔다.

Backend:

```text
POST /api/v1/spaces
```

Success result:

1. `CreateSpaceDialog`를 닫는다.
2. Space list를 갱신한다.
3. 생성된 space를 active space로 선택한다.
4. 새 space의 root children을 로드한다.
5. Active node와 editor selection은 empty state로 초기화한다.

Failure result:

1. Dialog를 닫지 않는다.
2. Validation 또는 server error를 form 안에 표시한다.
3. 기존 active space/editor 상태는 유지한다.

### 2.2 Rename space

Trigger:

```text
Settings 또는 Space management 화면에서 Rename action 수행
```

Backend:

```text
PATCH /api/v1/spaces/{space_id}
```

Success result:

1. Space list item의 name/sort order를 갱신한다.
2. Rename 대상이 active space이면 TitleBar/StatusBar의 current space 표시도 갱신한다.
3. Active node와 editor groups는 유지한다.

Failure result:

1. Rename form을 유지한다.
2. Field 또는 form error를 표시한다.

### 2.3 Delete space

Trigger:

```text
Settings 또는 Space management 화면에서 Delete action 수행
```

Backend:

```text
DELETE /api/v1/spaces/{space_id}
```

Success result:

1. 삭제된 space를 ActivityRail에서 제거한다.
2. 삭제된 space가 active space이면 active space/node/editor groups를 비운다.
3. 접근 가능한 다른 space가 있으면 사용자가 선택할 수 있게 한다.
4. 남은 space가 없으면 empty state와 Create Space CTA를 보여준다.

Failure result:

1. 삭제 confirm 화면을 유지하거나 닫고 error toast를 보여준다.
2. 기존 active space/editor 상태는 유지한다.

Constraints:

- 로그인 성공만으로 space를 자동 생성하지 않는다.
- `ActivityRail`의 `+`는 생성 진입점이지만, rename/delete를 inline으로 처리하지 않는다.
- Space 삭제는 soft delete다. UI는 즉시 목록에서 제거된 것으로 취급한다.

## 3. Select space

Trigger:

```text
사용자가 ActivityRail의 space item을 선택한다.
```

Backend:

```text
GET /api/v1/spaces/{space_id}
GET /api/v1/spaces/{space_id}/nodes/{root_node_id}/children?limit=100
```

UI result:

- active space context를 변경한다.
- `PrimarySidebar` tree를 해당 space root 기준으로 초기화한다.
- `EditorArea`는 이전 space의 선택을 유지하지 않는다. 필요하면 local recent state만 참고한다.
- `AuxiliarySidebar`는 active node가 없으면 empty inspector를 보여준다.

Constraints:

- caller가 볼 수 없는 space는 rail에 표시하지 않는다.
- Space 전환은 file/text content를 미리 모두 읽지 않는다.

## 4. Browse tree

Trigger:

```text
사용자가 TreeSection에서 folder를 펼치거나 접는다.
```

Backend:

```text
GET /api/v1/spaces/{space_id}/nodes/{folder_id}/children?limit=100&cursor=...
```

UI result:

- Folder row click은 expand/collapse를 토글한다.
- 펼친 folder만 children을 요청한다.
- 스크롤이 끝에 가까워지면 다음 cursor page를 요청한다.
- Tree selection은 현재 보이는 item 기준으로 이동한다.

Constraints:

- Children page 기본값은 100, 최대 200이다.
- Folder 하나의 direct children은 최대 1,000개다.
- 전체 tree를 한 번에 펼치거나 렌더링하지 않는다.
- TreeSection과 RecentSection은 독립 스크롤 영역이다.

## 5. Open node in editor

Trigger:

```text
사용자가 TreeSection 또는 RecentSection에서 node를 선택한다.
```

Backend:

```text
GET /api/v1/spaces/{space_id}/nodes/{node_id}
GET /api/v1/spaces/{space_id}/nodes/{node_id}/reveal

kind=text:
GET /api/v1/spaces/{space_id}/text/{node_id}

kind=file:
GET /api/v1/spaces/{space_id}/files/{node_id}
```

UI result:

- 선택한 node를 active `EditorGroup`에 연다.
- Tree가 선택 node의 부모 folder들을 아직 로드하지 않았다면 `reveal` 결과의 ancestor chain으로 필요한 folder를 펼친다.
- `InspectorPanel`은 같은 node의 metadata/stat를 표시한다.
- Text는 기본 preview mode로 연다.
- File은 metadata/stat와 preview 또는 download action을 보여준다.
- Folder는 folder summary를 보여준다.

Constraints:

- Node 선택은 새 `EditorGroup`을 자동 생성하지 않는다.
- 새 group은 TitleBar의 split/add action으로만 만든다.
- Text body와 file bytes는 node detail에 포함되지 않는다.
- File download는 명시적 action에서만 수행한다.

## 6. Node create: folder, text, file

Node create는 active space 안의 parent folder를 기준으로 수행한다.

### 6.1 Choose parent

Parent 결정:

1. Folder context menu에서 시작하면 해당 folder가 parent다.
2. Root/empty tree context에서 시작하면 active space root가 parent다.
3. `PrimarySidebar` header `+`에서 시작하면 현재 선택 folder 또는 active space root를 parent로 사용한다.
4. Text/File context에서는 child 생성 action을 제공하지 않는다.

### 6.2 Create folder

Backend:

```text
POST /api/v1/spaces/{space_id}/nodes
```

Request intent:

```text
kind = folder
parent_id
name
```

Success result:

1. Parent folder children을 갱신한다.
2. Parent folder가 접혀 있으면 펼칠 수 있다.
3. 새 folder row를 tree에서 selected 상태로 둘 수 있다.
4. EditorArea는 folder summary를 열거나 기존 editor를 유지한다.

### 6.3 Create text

Backend:

```text
POST /api/v1/spaces/{space_id}/nodes
```

Request intent:

```text
kind = text
parent_id
name
optional plain content
```

Success result:

1. Parent folder children을 갱신한다.
2. 생성된 Text를 active EditorGroup에 열 수 있다.
3. Text는 기본 preview mode로 열린다.
4. InspectorPanel은 새 node 정보를 표시한다.

Constraints:

- Text create 요청에서는 encrypted payload를 받지 않는다.
- Encrypted Text 저장은 생성 이후 `PUT /text/{node_id}` flow를 사용한다.

### 6.4 Upload file

Backend:

```text
POST /api/v1/spaces/{space_id}/files
```

Request intent:

```text
parent_node_id
name
file bytes
media_type?
original_filename?
encryption_mode?
encryption_metadata?
```

Success result:

1. Parent folder children을 갱신한다.
2. 생성된 File을 active EditorGroup에 열 수 있다.
3. File은 TextEditor가 아니라 file metadata/preview/download view로 열린다.
4. InspectorPanel은 file stats를 표시한다.

Constraints:

- 현재 File upload는 256 KiB 이하 inline file만 지원한다.
- File은 grep/search content 대상이 아니다.

### 6.5 Failure handling

Create 실패 시:

1. Dialog/form을 유지한다.
2. Parent tree cache를 변경하지 않는다.
3. Field error 또는 server error를 표시한다.
4. Active editor group은 기존 상태를 유지한다.

## 7. Edit and save text

Trigger:

```text
사용자가 Text node를 열고 Edit mode로 전환한다.
```

Backend:

```text
GET   /api/v1/spaces/{space_id}/text/{node_id}?start_line=...&max_lines=...&max_bytes=...
PUT   /api/v1/spaces/{space_id}/text/{node_id}
PATCH /api/v1/spaces/{space_id}/text/{node_id}
```

UI result:

- Preview mode가 기본이다.
- Edit mode에서 저장하면 저장 상태를 짧게 반영한다. Desktop은 `StatusBar`에, `StatusBar`를 숨기는 mobile은 일시 toast로 표시한다.
- 저장 성공 후 `InspectorPanel`의 byte/line/update 정보를 갱신한다.

Constraints:

- Text read는 line/byte 제한을 가진다. UI는 부분 응답을 전체 문서로 오해하지 않아야 한다.
- Plain Text만 server-side preview/edit/patch 대상이다.
- Encrypted Text는 서버가 복호화하지 않는다. UI는 encrypted payload 상태를 명확히 보여준다.
- `.json`, `.jsonl`, `.yaml`, `.yml`, `.toml` 이름의 plain Text는 저장 후 문법 검증 대상이다.
- Conflict 방지를 위해 가능하면 `expected_sha256`을 사용한다.

## 8. Upload and download file

Trigger:

```text
사용자가 Upload File 또는 File download action을 수행한다.
```

Backend:

```text
POST /api/v1/spaces/{space_id}/files
GET  /api/v1/spaces/{space_id}/files/{node_id}
GET  /api/v1/spaces/{space_id}/files/{node_id}/content
```

UI result:

- Upload 성공 시 parent folder children을 갱신한다.
- File node를 열면 metadata/stat를 보여준다.
- Download action은 stored bytes를 그대로 내려받는다.

Constraints:

- File은 TextEditor에서 직접 수정하지 않는다.
- `encryption_mode=client` file은 서버가 복호화하지 않는다.
- File은 grep/search content 대상이 아니다.
- 256 KiB 초과 upload는 현재 API에서 거부한다.

## 9. Edit metadata

Trigger:

```text
사용자가 InspectorPanel에서 node metadata를 수정한다.
```

Backend:

```text
GET   /api/v1/spaces/{space_id}/nodes/{node_id}/metadata
PUT   /api/v1/spaces/{space_id}/nodes/{node_id}/metadata
PATCH /api/v1/spaces/{space_id}/nodes/{node_id}/metadata
```

UI result:

- Metadata 수정 후 active node detail과 InspectorPanel을 갱신한다.
- Tree label은 node name을 기준으로 유지한다. Metadata title이 tree name을 자동 대체하지 않는다.

Constraints:

- Metadata는 JSON object다.
- Metadata는 Text/File content 암호화 대상이 아니다.
- 민감한 값은 metadata에 넣지 않는다는 안내를 유지한다.

## 10. Node update and delete

Node update는 tree resource를 바꾼다. Text/File content body 수정은 Text/File category flow에서 다룬다.

### 10.1 Rename node

Trigger:

```text
Tree context menu, EditorGroupHeader, 또는 Inspector action에서 Rename 수행
```

Backend:

```text
PATCH /api/v1/spaces/{space_id}/nodes/{node_id}
```

Success result:

1. 해당 node detail을 갱신한다.
2. Parent folder children을 갱신한다.
3. 열린 EditorGroup header의 name을 갱신한다.
4. InspectorPanel의 path/name을 갱신한다.
5. Active node는 유지한다.

Constraints:

- Rename은 content body를 수정하지 않는다.
- Root node는 rename할 수 없다.

### 10.2 Reorder node

Trigger:

```text
Tree reorder UI가 생긴 경우 sort_order 변경
```

Backend:

```text
PATCH /api/v1/spaces/{space_id}/nodes/{node_id}
```

Success result:

1. Parent folder children을 갱신한다.
2. Tree order를 backend 정렬 결과에 맞춘다.

Constraints:

- 현재 문서에서는 reorder UI를 필수 화면으로 정의하지 않는다.
- sort_order는 backend resource state다.

### 10.3 Move node

Trigger:

```text
Tree context menu 또는 move dialog에서 parent 변경
```

Backend:

```text
POST /api/v1/spaces/{space_id}/nodes/{node_id}/move
```

Success result:

1. Old parent children을 갱신한다.
2. New parent children을 갱신한다.
3. 열린 EditorGroup이 해당 node를 보고 있으면 계속 유지한다.
4. InspectorPanel의 path를 갱신한다.
5. TreeSection은 새 위치 reveal을 시도한다.

Constraints:

- Move는 같은 space 안에서만 가능하다.
- Root node는 move할 수 없다.

### 10.4 Delete node

Trigger:

```text
Tree context menu, EditorGroupHeader, 또는 Inspector action에서 Delete 수행
```

Backend:

```text
DELETE /api/v1/spaces/{space_id}/nodes/{node_id}?recursive=true
```

Success result:

1. Parent folder children을 갱신한다.
2. 삭제된 node를 Tree/Recent에서 제거한다.
3. 삭제된 node가 editor group에 열려 있으면 해당 group을 empty state로 바꾼다.
4. 삭제된 node가 tree selection이면 selection을 해제한다.
5. InspectorPanel은 empty state를 보여준다.

Constraints:

- Folder 삭제에는 `recursive=true`와 사용자 confirm이 필요하다.
- 삭제는 soft delete다.
- Root node는 delete할 수 없다.
- 동기 folder delete는 최대 1,000 nodes까지만 처리한다.

### 10.5 Failure handling

Update/delete 실패 시:

1. 기존 tree/editor state를 유지한다.
2. 실패한 form/dialog 또는 toast에 error를 표시한다.
3. Optimistic UI를 적용했다면 backend 정본으로 rollback한다.

## 11. EditorGroup management

Trigger:

```text
사용자가 TitleBar의 split/add action 또는 EditorGroup close action을 사용한다.
```

Backend:

```text
none for split/close itself
```

UI result:

- `EditorGroup`은 1개에서 최대 3개까지 표시한다.
- 새 group은 active group의 오른쪽에 추가한다.
- group close는 local UI state만 변경한다.

Constraints:

- Split은 preview/edit 동시 표시가 아니다.
- 각 group은 열린 node와 preview/edit mode를 독립적으로 가진다.
- 최대 group 수에 도달하면 add action은 disabled 상태가 된다.

## 12. Auxiliary inspector view

Trigger:

```text
사용자가 AuxiliarySidebar를 연다.
```

Backend:

```text
Inspector: current node REST response and metadata endpoints
```

UI result:

- Inspector는 active node의 kind/path/metadata/stat/attribution을 보여준다.

Constraints:

- EditorGroup 안에 중복 inspector button을 두지 않는다.

## 13. Mobile presentation

Trigger:

```text
사용자가 tablet/mobile viewport에서 대시보드를 사용한다.
```

Backend:

```text
same REST API as desktop
```

규칙:

- Mobile/tablet presentation의 정본은 `02-layout.md`의 Viewport presentations를 따른다. Flow는 viewport와 무관하게 같은 REST API를 사용한다.
- Layout role 이름은 desktop과 동일하게 유지하고 presentation만 바꾼다.
- Touch 사용자는 context menu 기능을 header action이나 overflow menu로도 접근할 수 있어야 한다.

## Not supported in dashboard v1

현재 flow로 정의하지 않는 것:

```text
global command/search box
semantic search screen
MCP sequence execution UI
file-level ACL/chmod
cross-space move/copy
byte-offset editor
preview/edit split view
server-side full file search
bottom panel / terminal / task output
```

이 항목들은 backend 계약이 확정되기 전까지 UI 기본 흐름에 넣지 않는다.
