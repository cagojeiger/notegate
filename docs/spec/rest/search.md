# REST Search

```http
GET /api/v1/spaces/{space_id}/search/find?q=note&limit=50&cursor=...
GET /api/v1/spaces/{space_id}/search/grep?q=todo&path=/notes&limit=50&cursor=...
```

`find`는 node name metadata를 검색한다. Folder/Text/File 모두 대상이다.

`grep`은 Text content만 검색한다. File은 대상이 아니다. Encrypted Text는 별도 검색 index가 없으면 grep 대상이 아니다.
