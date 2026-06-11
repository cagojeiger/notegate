# MCP Search

MCP search는 Space path를 기준으로 folder subtree를 탐색한다. 공통 traversal, cursor, memory budget은 `../search.md`를 따른다.

## `files_find`

Node name을 검색한다.

```json
{"target":"personal:/notes","q":"state","kind":"text","match":"contains","limit":50}
```

입력 필드:

```text
target 또는 selector+path
q       필수
kind    선택, folder|text|file
match   contains|regex|glob, 기본 contains
limit   기본 50, 최대 100
cursor  선택
```

Folder/Text/File 모두 대상이다. Root node는 결과에서 제외한다. Content와 metadata는 검색하지 않는다.

## `files_grep`

Query를 포함하는 plain Text node 후보를 검색한다.

```json
{"target":"personal:/memory","q":"todo","match":"literal","limit":20}
```

입력 필드:

```text
target 또는 selector+path
q        필수
match    literal|regex, 기본 literal
include  선택, path glob list
exclude  선택, path glob list
limit    기본 20, 최대 100
cursor   선택
```

응답은 match line이 아니라 Text node 후보 목록이다. File, encrypted Text, metadata는 대상이 아니다. Match된 Text의 내용은 `files_read`로 조회한다.
