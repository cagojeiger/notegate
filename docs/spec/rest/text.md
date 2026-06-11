# REST Text

Text는 UTF-8 content node다. read/write/patch/grep 대상이다.

```http
GET   /api/v1/spaces/{space_id}/text/{node_id}?start_line=1&max_lines=200&max_bytes=65536
PUT   /api/v1/spaces/{space_id}/text/{node_id}
PATCH /api/v1/spaces/{space_id}/text/{node_id}
```

Request body examples:

```json
{"content":"# note\n"}
```

```json
{"expected_sha256":"...","edits":[{"old":"hello","new":"hi"}]}
```

Rules:

- `node_id`는 `nodes.kind='text'`여야 한다.
- Content는 UTF-8이다.
- Patch는 exact-match 방식이며 각 `old`는 정확히 한 번만 매칭되어야 한다.
- `expected_sha256`이 있으면 현재 plaintext hash와 일치해야 한다.
- Encrypted Text도 서버가 복호화 가능한 경우 read/write/patch 가능하다.
