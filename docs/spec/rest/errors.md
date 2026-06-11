# REST Error policy

## Error policy

REST 오류 응답은 항상 같은 기본 shape을 사용한다.

```json
{
  "error": "invalid_input",
  "kind": "invalid_input",
  "message": "human readable message"
}
```

`error`와 `kind`는 같은 값을 가진다. `kind`는 MCP `data.kind`와 같은 의미의 공통 분류다.

```text
invalid_input  -> 400 invalid field/name/path, malformed limit, malformed/tampered cursor
forbidden      -> 403 authenticated but not allowed
not_found      -> 404 not found or cross-space hidden resource
conflict       -> 409 state conflict, quota conflict, stale hash, duplicate destination, subtree too large
internal_error -> 500 redacted internal error
```

Auth middleware 오류도 같은 기본 shape을 사용한다. `not_registered`는 client onboarding을 위해 `login_url`과 `mcp_url`을 추가로 포함한다.

```text
missing_token    -> 401 missing/malformed auth
invalid_token    -> 401 invalid auth
not_registered   -> 403 authenticated but no active local account
inactive_account -> 403 inactive OAuth/session local account
```

API key가 비활성 account에 연결되어 있으면 credential 존재를 노출하지 않기 위해 `401 invalid_token`으로 처리한다.
