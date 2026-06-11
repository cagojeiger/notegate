# MCP Spaces

## `spaces_list`

인증된 caller가 접근 가능한 space 목록을 반환한다.

```json
{"limit":50,"cursor":"opaque"}
```

User caller는 자신이 소유한 spaces를 본다. Agent caller는 연결된 spaces를 본다. 정렬은 `sort_order ASC, name ASC` 뒤 내부 tie-breaker로 안정화하며 `cursor`는 opaque 값이다.

## `spaces_create`

User caller가 space를 만든다.

```json
{"name":"personal"}
```

Agent caller는 space를 만들 수 없다.

## `spaces_get`

Space name으로 space 하나를 반환한다.

```json
{"name":"personal"}
```

MCP는 space UUID 입력을 받지 않는다. Space name이 중복되어 ambiguity가 나면 name을 정리해야 한다.
