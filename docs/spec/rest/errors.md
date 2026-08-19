# REST Error policy

## Error policy

REST handler와 인증 middleware가 반환하는 오류는 같은 기본 shape을 사용한다. 공개 V2는 extractor, body limit, timeout, rate limit에서 발생한 transport 오류도 이 shape으로 정규화한다.

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
method_not_allowed -> 405 unsupported HTTP method on a V2 resource
request_timeout -> 408 request processing deadline exceeded
conflict       -> 409 state conflict, quota conflict, stale hash, duplicate destination, subtree too large
payload_too_large -> 413 request body limit exceeded
node_write_locked    -> 423 target node or an ancestor is write-locked
subtree_write_locked -> 423 source subtree contains a directly write-locked node
rate_limited   -> 429 process-wide HTTP capacity exceeded
usage_reconciliation_cooldown -> 409 reconciliation completed within the cooldown window
internal_error -> 500 redacted internal error
usage_recalculation_in_progress -> 503 temporary read-only maintenance
```

Retry 가능한 REST 임시 오류는 HTTP `Retry-After` header를 반환할 수 있다. MCP 오류의 `data.retryable`과 `data.retry_after_seconds`는 MCP 계약에만 속한다. Usage reconciliation 응답은 `../usage-and-quotas.md`를 따른다.

Auth middleware 오류도 같은 기본 shape을 사용한다. `not_registered`는 client onboarding을 위해 `login_url`과 `mcp_url`을 추가로 포함한다.

```text
missing_token    -> 401 missing/malformed auth
invalid_token    -> 401 invalid auth
not_registered   -> 403 authenticated but no active local account
inactive_account -> 403 inactive OAuth/session local account
```

API key가 비활성 account에 연결되어 있으면 credential 존재를 노출하지 않기 위해 `401 invalid_token`으로 처리한다.
