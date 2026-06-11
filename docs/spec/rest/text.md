# REST Text

Text는 UTF-8 content node다. plain Text는 read/write/patch/grep 대상이고, encrypted Text는 client-side encrypted payload로 저장/조회한다.

```http
GET   /api/v1/spaces/{space_id}/text/{node_id}?start_line=1&max_lines=200&max_bytes=65536&if_none_match_sha256=...
PUT   /api/v1/spaces/{space_id}/text/{node_id}
PATCH /api/v1/spaces/{space_id}/text/{node_id}
```

Request body examples:

```json
{"storage_format":"plain","content":"# note\n"}
```

```json
{"storage_format":"encrypted","encrypted_payload":{"version":1,"alg":"AES-256-GCM","ciphertext_b64":"..."}}
```

```json
{"expected_sha256":"...","edits":[{"old_text":"hello","new_text":"hi"}]}
```

Rules:

- `node_id`는 `nodes.kind='text'`여야 한다.
- `storage_format`은 `plain` 또는 `encrypted`다. 생략하면 `plain`이다.
- plain Content는 UTF-8이다.
- encrypted payload는 JSON object이며 서버가 복호화하지 않는다.
- Patch는 plain Text 전용 exact-match 방식이며 각 `old_text`는 정확히 한 번만 매칭되어야 한다.
- `expected_sha256`이 있으면 저장된 content hash와 일치해야 한다.
- `if_none_match_sha256`이 저장된 content hash와 일치하면 content body 대신 `unchanged=true` 응답을 반환한다.
- encrypted Text는 grep/patch 대상이 아니다.
