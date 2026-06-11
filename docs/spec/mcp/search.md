# MCP Search

## `files_find`

Node name metadata를 검색한다.

```json
{"target":"personal:/notes","q":"state","limit":50}
```

Folder/Text/File 모두 대상이다. Root node는 결과에서 제외한다.

## `files_grep_text`

Text content를 검색한다.

```json
{"target":"personal:/memory","q":"todo","limit":50}
```

File은 대상이 아니다. Encrypted Text는 별도 search index가 없으면 대상이 아니다.
