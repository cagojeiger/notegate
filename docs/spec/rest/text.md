# REST Text

Text는 content node다. plain Text는 read/write/patch 대상이고, encrypted Text는 client-side encrypted payload로 저장/조회한다.

```http
GET   /api/v1/spaces/{space_id}/text/{node_id}?start_line=1&max_lines=200&max_bytes=65536&if_none_match_sha256=...
PUT   /api/v1/spaces/{space_id}/text/{node_id}
PATCH /api/v1/spaces/{space_id}/text/{node_id}
```

공통 schema는 `../schemas.md`를 따른다.

## Response shapes

```ts
GET   /text/{node_id} -> RestTextReadResponse
PUT   /text/{node_id} -> { node: NodeRef, text: TextMeta }
PATCH /text/{node_id} -> { node: NodeRef, text: PatchTextMeta }
```

```ts
type TextMeta = {
  node_id: string
  storage_format: "plain" | "encrypted"
  content_sha256: string
  byte_len: number
  line_count: number
  updated_by: AccountRef
  updated_at: string
}

type PatchTextMeta = TextMeta & {
  previous_sha256: string
  edits_applied: number
  diff: string
}
```

## Request body examples

```json
{"storage_format":"plain","content":"# note\n"}
```

```json
{"storage_format":"encrypted","encrypted_payload":{"version":1,"alg":"AES-256-GCM","ciphertext_b64":"..."}}
```

```json
{"expected_sha256":"...","edits":[{"old_text":"hello","new_text":"hi","mode":"unique","expected_count":1}]}
```

## GET rules

- `node_id`는 `nodes.kind='text'`여야 한다.
- plain Text는 `start_line`, `max_lines`, `max_bytes`를 적용해 content slice를 반환한다.
- encrypted Text는 line slicing을 적용하지 않고 encrypted payload 전체를 반환한다.
- `if_none_match_sha256`이 저장된 content hash와 일치하면 content body 대신 `unchanged=true` 응답을 반환한다.

## PUT/PATCH rules

- `node_id`는 `nodes.kind='text'`여야 한다.
- `storage_format`은 `plain` 또는 `encrypted`다. 생략하면 `plain`이다.
- plain content는 UTF-8이다.
- encrypted payload는 JSON object이며 서버가 복호화하지 않는다.
- `PUT`은 plain/encrypted 전체 교체를 수행한다.
- `PATCH`는 plain Text 전용 string replacement 방식이다. 기본 `mode`는 `unique`이며 각 `old_text`는 정확히 한 번만 매칭되어야 한다. `first`와 `all` mode를 명시할 수 있다.
- `expected_count`가 있으면 현재 match 수와 일치해야 한다.
- `expected_sha256`이 있으면 저장된 content hash와 일치해야 한다.
- encrypted Text는 patch 대상이 아니다.
