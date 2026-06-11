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
stat    folder/text/file metadata 조회
mkdir   folder 생성
touch   empty text 생성
mv      node rename/move
rm      node soft delete
find    node name metadata 검색
```

공통 권한:

```text
read  permission -> ls/stat/find 가능
write permission -> mkdir/touch/mv/rm 가능
```

User caller는 자신이 소유한 space에서 read/write/manage 가능하다. Agent caller는 connection permission에 따른다.

## Text commands

```text
read   text content 읽기
write  text content 전체 쓰기
patch  exact-match patch 적용
grep   text content 검색
```

- Text는 UTF-8 content다.
- `read`, `write`, `patch`, `grep`는 `nodes.kind='text'`에만 적용한다.
- `patch`는 각 edit의 `old_text` 문자열이 원문에서 정확히 한 번만 매칭되어야 한다.
- 여러 edit은 원본 기준으로 검증한 뒤 적용한다.
- `expected_sha256`이 주어지면 현재 content hash와 일치해야 한다.

## File commands

File command schema는 향후 binary/object content를 위해 예약되어 있다. 현재 MCP/CLI command surface에는 upload/download를 노출하지 않는다.

- File은 `nodes.kind='file'`이다.
- File은 text read/patch/grep 대상이 아니다.

## Search semantics

```text
find      folder/text/file node name 검색
grep     text_objects content 검색
```

Encrypted Text는 현재 grep 대상이 아니다.
