# REST Access

## Access

Access API는 workspace owner-only endpoint다. Owner는 `workspace_access.role='owner'`인 active user account다. Access row는 owner/editor/viewer membership을 저장하며, `workspaces.created_by`는 최초 생성자/audit attribution이다. Grant/revoke/downgrade side effect와 owner 보호 규칙은 `docs/spec/lifecycle.md`를 따른다. 한 workspace는 자동 owner row를 포함해 최대 `20`개의 active access row를 가진다.

Live access는 다음 조건을 모두 만족해야 한다.

```text
workspace_access.revoked_at IS NULL
accounts.is_active = true
accounts.deleted_at IS NULL
```

비활성화/삭제된 account는 기존 access row가 남아 있어도 권한으로 인정하지 않고 access list에도 표시하지 않는다. 비활성화/삭제된 account에 새 access를 grant하는 것도 거부한다. Agent account는 `owner` role을 받을 수 없다.

### List access

```http
GET /api/v1/workspaces/{workspace_id}/access?limit=100&cursor=...
```

Live access 목록을 반환한다. Owner row도 목록에 포함된다. Default/max limit은 `100`이다.

### Grant or change access

```http
PUT /api/v1/workspaces/{workspace_id}/access/{account_id}
```

```json
{
  "role": "viewer"
}
```

대상 account가 active 상태일 때 `viewer`, `editor`, 또는 `owner` access를 생성하거나 role을 변경한다. `owner` role은 active user account에만 허용하고 agent account에는 허용하지 않는다. 이미 revoke된 row가 있으면 같은 `(workspace_id, account_id)` row를 다시 활성화한다. 이때 현재 grant 상태의 `granted_by`/`granted_at`을 갱신한다. active access row가 `20`개를 넘으면 `409 conflict`로 거부한다. Creator owner row와 마지막 active user owner 보호는 `docs/spec/lifecycle.md`를 따른다.

### Revoke access

```http
DELETE /api/v1/workspaces/{workspace_id}/access/{account_id}
```

대상 access에 `revoked_at`/`revoked_by`를 설정한다. 현재 grant attribution field는 그대로 유지한다. Creator owner row와 마지막 active user owner는 일반 Access API로 revoke할 수 없으며 `409 conflict`로 거부한다. 이미 live grant가 없는 account를 revoke하는 요청은 caller owner check 후 성공으로 처리한다.
