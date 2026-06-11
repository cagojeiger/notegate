# MCP Search

## `files_find`

Node name metadata를 검색한다.

```json
{"target":"personal:/notes","q":"state","limit":50}
```

Folder/Text/File 모두 대상이다. Root node는 결과에서 제외한다.

## `files_grep`

Plain Text content를 검색한다.

```json
{"target":"personal:/memory","q":"todo","limit":50}
```

File과 encrypted Text는 대상이 아니다.
