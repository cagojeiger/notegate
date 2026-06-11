# File tree commands

이 문서는 Space 안 tree command의 공통 semantics다. MCP와 CLI는 이 모델을 path-first로 노출하고, REST는 id-first resource API로 노출한다.

## Path model

```text
space:/path/to/item
```

- Root path는 `/`다.
- Full path는 저장하지 않고 parent chain에서 derive한다.
- 같은 folder 안 live node name은 unique다.
- Node kind는 `folder`, `text`, `file`이다.

## Common commands

```text
ls      folder children 목록
stat    folder/text/file 상태 조회
mkdir   folder 생성
touch   empty text 생성
mv      node rename/move
rm      node soft delete
find    node name/kind/scope 검색
grep    plain Text 후보 검색
```

공통 권한:

```text
read  permission -> ls/stat/read text/find/grep 가능
write permission -> read + mkdir/touch/write/patch/mv/rm 가능
```

User caller는 자신이 소유한 space에서 read/write/manage 가능하다. Agent caller는 connection permission에 따른다.

## Text commands

```text
read   text content 읽기
write  text content 전체 쓰기
patch  exact-match patch 적용
```

- Text command는 plain UTF-8 content를 대상으로 한다.
- `read`, `write`, `patch`는 `nodes.kind='text'`에만 적용한다.
- Encrypted Text content는 MCP/CLI read/write/patch 대상이 아니다.
- `patch`는 각 edit의 `old_text` 문자열이 원문에서 정확히 한 번만 매칭되어야 한다.
- 여러 edit은 원본 기준으로 검증한 뒤 적용한다.
- `expected_sha256`이 주어지면 저장된 content hash와 일치해야 한다.

## File commands

File은 binary/object content node다. MCP/CLI command surface는 file upload/download를 포함하지 않고 file node stat만 노출한다. Node metadata는 REST metadata API에서 다룬다.

- File은 `nodes.kind='file'`이다.
- File은 text read/patch/grep 대상이 아니다.

## Search semantics

Search는 MCP/CLI용 command semantics다. REST resource API는 search endpoint를 제공하지 않는다. 세부 traversal, cursor, memory budget은 `search.md`를 따른다.

```text
find        folder/text/file node name/kind/scope 검색
grep        plain Text content가 query를 포함하는 text node 후보 검색
```

`grep`은 기본적으로 Text node 후보만 반환한다. 요청 옵션으로 첫 matching line 또는 모든 matching line number를 받을 수 있다. Context와 snippet은 반환하지 않는다. Match된 Text 내용은 `read`로 조회한다. Encrypted Text와 File은 grep 대상이 아니다.
