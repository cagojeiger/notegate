# MCP Search

## `files_find`

Node name을 검색한다.

```json
{"target":"personal:/notes","q":"state","kind":"text","limit":50}
```

Folder/Text/File 모두 대상이다. Root node는 결과에서 제외한다. Node metadata 내용은 검색하지 않는다.
입력 필드: `target`(또는 `selector`+`path`), `q`(필수), `kind`(선택 `folder|text|file` 필터), `limit`(기본 50), `cursor`. `kind=file`은 File surface에 포함된 node만 매칭한다.

## `files_grep`

Plain Text content를 검색한다.

```json
{"target":"personal:/memory","q":"todo","context":2,"limit":20}
```

File과 encrypted Text는 대상이 아니다. Node metadata 내용은 검색하지 않는다.
입력 필드: `target`(또는 `selector`+`path`), `q`(필수), `context`(선택, match 당 주변 줄 수), `limit`(기본 20), `cursor`.
