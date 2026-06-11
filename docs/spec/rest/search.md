# REST Search

```http
POST /api/v1/spaces/{space_id}/search/find
POST /api/v1/spaces/{space_id}/search/grep
```

Request examples:

```json
{"q":"note","limit":50,"cursor":"..."}
```

```json
{"q":"todo","path":"/notes","limit":50,"cursor":"..."}
```

`find`는 node name metadata를 검색한다. Folder/Text/File 모두 대상이다.

`grep`은 plain Text content만 검색한다. File과 encrypted Text는 대상이 아니다.
