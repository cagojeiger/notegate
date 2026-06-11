# MCP Spaces

## `spaces_list`

인증된 caller가 접근 가능한 space 목록을 반환한다.

```json
{"limit":50,"cursor":"opaque"}
```

User caller는 자신이 소유한 spaces를 본다. Agent caller는 연결된 spaces를 본다. 정렬은 `sort_order ASC, name ASC, id ASC`이며 `cursor`는 opaque 값이다.

## `spaces_create`

User caller가 space를 만든다.

```json
{"name":"personal"}
```

Agent caller는 space를 만들 수 없다.

## `spaces_get`

Selector로 space 하나를 반환한다.

```json
{"space":"personal"}
```

```json
{"space_id":"space-id"}
```

Selector 생략 시 visible space가 정확히 하나이면 그 space를 선택한다. 여러 개면 ambiguity error를 반환한다.
