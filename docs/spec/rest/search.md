# REST Search

```http
POST /api/v1/spaces/{space_id}/search/find
POST /api/v1/spaces/{space_id}/search/grep
```

Request examples:

```json
{"q":"note","path":"/notes","kind":"text","limit":50,"cursor":"..."}
```

```json
{"q":"todo","path":"/notes","context":2,"limit":50,"cursor":"..."}
```

`find`는 node name metadata를 검색한다. Folder/Text/File 모두 대상이다.
Body 필드: `q`(필수), `path`(선택 scope), `kind`(선택 `folder|text|file` 필터), `limit`(기본 50), `cursor`. `kind=file`은 File surface에 포함된 node만 매칭한다.

`grep`은 plain Text content만 검색한다. File과 encrypted Text는 대상이 아니다.
Body 필드: `q`(필수), `path`(선택 scope), `context`(선택, match 당 주변 줄 수), `limit`(기본 20), `cursor`.
