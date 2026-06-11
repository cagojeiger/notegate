# REST Text

Text는 UTF-8 content node다. read/write/patch/grep 대상이다.

```http
GET   /api/v1/spaces/{space_id}/text/{node_id}?start_line=1&max_lines=200&max_bytes=65536&if_none_match_sha256=...
PUT   /api/v1/spaces/{space_id}/text/{node_id}
PATCH /api/v1/spaces/{space_id}/text/{node_id}
```

Request body examples:

```json
{"content":"# note\n"}
```

```json
{"expected_sha256":"...","edits":[{"old_text":"hello","new_text":"hi"}]}
```

Rules:

- `node_id`는 `nodes.kind='text'`여야 한다.
- Content는 UTF-8이다.
- Patch는 exact-match 방식이며 각 `old_text`는 정확히 한 번만 매칭되어야 한다.
- `expected_sha256`이 있으면 저장된 plaintext hash와 일치해야 한다.
- `if_none_match_sha256`이 저장된 content hash와 일치하면 content body 대신 `unchanged=true` 응답을 반환한다.
- REST text read/write/patch는 plain Text만 대상으로 한다.
