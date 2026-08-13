# File tree commands

이 문서는 Space 안 tree command의 공통 semantics다. MCP와 CLI는 `target` 중심 path-first 모델로 노출하고, REST는 id-first resource API로 노출한다.

## Path model

```text
target = space:/path/to/item
```

- Root path는 `/`다.
- Full path는 저장하지 않고 parent chain에서 derive한다.
- 같은 folder 안 live node name은 unique다.
- Node name은 1~128자 Unicode 문자열이다. 한글과 내부 공백은 허용한다. `/`, control char, 앞뒤 공백, `.`, `..`는 허용하지 않는다.
- Node kind는 `folder`, `text`, `file`이다.

## Common commands

```text
list    folder children 또는 subtree 목록
stat    folder/text/file 상태 조회
mkdir   folder 생성
mv      node rename/move
copy    node 복사
rm      node soft delete
find    node name/kind/scope 검색
grep    plain Text 후보 검색
```

공통 권한:

```text
read  permission -> list/stat/read text/find/grep 가능
write permission -> read + mkdir/write/append/patch/edit/mv/copy/rm 가능
```

User caller는 자신이 소유한 space에서 read/write/manage 가능하다. Agent caller는 connection permission에 따른다.

## Write lock

`nodes.write_locked=true`는 직접 설정된 node와 모든 live descendant에 쓰기 장벽을 만든다. 상속 상태를 별도 row로 저장하지 않고, mutation transaction 안에서 대상의 parent chain을 확인한다.

- 잠긴 node 또는 잠긴 조상 아래에서는 create, Text write/append/patch/edit, metadata, 검색 정책, 저장 Text 암호화, rename, reorder, move, delete, File upload 등록을 거부한다.
- 이동은 source와 destination 모두 확인한다. 잠긴 descendant가 있는 folder는 rename, move, delete할 수 없다.
- Copy는 source를 읽을 수 있지만 잠긴 destination에는 만들 수 없다. 복사본에는 직접 잠금을 복제하지 않는다.
- Read, metadata 조회, find/grep/tree, File download는 허용한다. Preview MIME 감지값처럼 read path가 기록하는 파생 File metadata도 content/tree 변경이 아니므로 허용한다. 같은 parent/name으로의 move, 현재 값과 같은 update, 이미 attach된 File upload의 완료 재시도처럼 실제 변경이 없는 요청도 허용한다.
- File upload 등록은 destination의 잠금을 검사하고 해당 upload handle의 write-lock 허가를 예약한다. 등록 뒤 destination이 잠겨도 기존 handle은 attach할 수 있으며, 잠금 이후 새 upload 등록은 거부한다. 완료 시 일반 write permission, parent 존재, 이름 충돌과 quota는 다시 확인한다. 취소되거나 만료된 upload object는 cleanup worker가 backoff로 정리한다.
- 잠금 설정/해제는 Dashboard Browser의 Space owner User 전용 보안 설정이다. MCP command는 이를 변경하지 않는다.
- 직접 잠금의 설정/해제는 상속된 잠금과 독립적으로 처리한다. 따라서 잠긴 조상 아래의 직접 잠금도 owner가 해제할 수 있고, effective lock은 남아 있는 직접 source가 없을 때만 풀린다.
- Space 삭제는 node mutation이 아닌 owner의 별도 resource lifecycle 작업이므로 node write lock이 막지 않는다.

## Text commands

```text
read   text content 읽기
write  text content 전체 교체 (`>`)
append text content 끝에 추가 (`>>`)
patch  string replacement 적용
edit   line 기반 insert/replace/delete 적용
```

- Text command는 plain UTF-8 content를 대상으로 한다.
- `read`, `write`, `append`, `patch`, `edit`은 `nodes.kind='text'`에만 적용한다.
- Client-side encrypted Text content는 MCP read/write/append/patch/edit 대상이 아니다.
- `write`는 전체 content replacement다.
- 빈 Text 생성은 `write`에 `create=true`, `content=""`를 사용한다.
- `append`는 기본적으로 정확한 EOF append다. 줄 구분이 필요하면 호출자가 newline을 포함하거나 `ensure_newline` 옵션을 사용한다.
- `patch`는 string replacement다. 기본 mode는 `unique`이며 `old_text`가 정확히 한 번만 매칭되어야 한다.
- `patch`는 명시적으로 `first` 또는 `all` mode를 사용할 수 있다. `expected_count`가 있으면 현재 match 수와 일치해야 한다.
- `edit`은 1-based line operation이다. `insert_before_line`, `insert_after_line`, `replace_lines`, `delete_lines`를 지원한다. insert/replace `content`는 논리적인 줄 내용으로 해석되며 trailing newline이 없어도 줄 경계를 보존한다.
- 여러 patch/edit은 원본 기준으로 검증한 뒤 적용한다.
- `.json`, `.jsonl`, `.yaml`, `.yml`, `.toml` Text는 service layer에서 `write`/`append`/`patch`/`edit`의 최종 plain content를 저장하기 전에 문법 검증한다. REST와 MCP는 이 공통 규칙을 공유한다.
- 구조화 Text 검증은 file name extension 기준이며 schema validation은 하지 않는다.
- Markdown Text의 leading YAML frontmatter는 plain Text content의 일부이며 Node metadata로 변환하지 않는다.
- `expected_sha256`이 주어지면 저장된 content hash와 일치해야 한다.

## File commands

File은 binary/object content node다. MCP `file_transfer`는 presigned URL을 발급하고 실제 bytes는 로컬 caller와 S3 호환 저장소 사이에서 직접 전송한다. Node metadata는 REST metadata API에서 다룬다.

- File은 `nodes.kind='file'`이다.
- File은 Text content operation과 grep 대상이 아니다.
- 100MiB 이하는 single PUT, 초과 파일은 64MiB part의 multipart upload를 사용한다.
- 단일 file hard max는 100GiB이며 실제 허용량은 Space tier quota가 더 낮을 수 있다.
- MCP 업로드와 다운로드 URL은 `file_transfer` 응답에 임시로 노출되며 5분 뒤 만료된다. REST/browser URL의 만료 정책은 REST File 스펙을 따른다.
- Multipart part URL은 최대 16개씩 발급하고 재발급할 때 upload activity를 갱신한다.
- Multipart 호환 검증 대상은 MinIO다. S3 호환 provider는 create/upload-part/complete/abort 동작을 지원해야 한다.

## Copy semantics

`copy`는 같은 Space 안에서 source node를 destination path로 복사한다.

- Destination은 새 path여야 하며 overwrite하지 않는다.
- Folder 복사는 `recursive=true`가 필요하다.
- 새 root node와 descendants는 새 id를 가진다.
- Node metadata와 plain/encrypted Text payload는 보존한다.
- 새 row의 create/update attribution은 copy caller로 기록한다.
- Space 간 copy는 지원하지 않는다.
- File node 또는 File을 포함한 subtree 복사는 지원하지 않는다.

## Search semantics

Search는 MCP와 Public V2 REST가 공유하는 command semantics다. Browser V1 REST는 search endpoint를 제공하지 않는다. 세부 traversal, cursor, memory budget은 `search.md`를 따른다.

```text
find        folder/text/file node name/kind/scope 검색
grep        plain Text content가 query를 포함하는 text node 후보 검색
```

`grep`은 기본적으로 Text node 후보만 반환한다. 요청 옵션으로 첫 matching line 또는 모든 matching line number를 받을 수 있다. Context와 snippet은 반환하지 않는다. Match된 Text 내용은 `read`로 조회한다. Client-side encrypted Text와 File은 grep 대상이 아니다. 서버 관리 방식으로 at-rest 암호화된 plain Text는 복호화 후 검색한다.


## list

`list`는 선택 folder 아래 목록을 반환한다. 기본 `depth=1`은 direct children만 반환하고, `depth>1`은 subtree를 DFS pre-order로 반환한다. MCP의 path-first 구조 조회이며, REST의 node children API와 1:1 대응하지 않는다. 최소 depth는 1, 최대 Space path depth다.
