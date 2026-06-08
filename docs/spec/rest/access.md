# REST Access

## Access

Access API는 workspace owner-only endpoint다. Owner는 user 또는 agent account에 workspace 접근 권한을 부여하거나 회수할 수 있다. 한 workspace는 최대 `20`개의 active access account를 가진다. 최소 하나의 live `owner`는 항상 남아야 한다.

Live access는 다음 조건을 모두 만족해야 한다.

```text
workspace_access.revoked_at IS NULL
accounts.is_active = true
accounts.deleted_at IS NULL
```

비활성화/삭제된 account는 기존 access row가 남아 있어도 권한으로 인정하지 않고 access list에도 표시하지 않는다. 비활성화/삭제된 account에 새 access를 grant하는 것도 거부한다.

### List access

```http
GET /api/v1/workspaces/{workspace_id}/access?limit=100&cursor=...
```

Live access 목록을 반환한다. Default/max limit은 `100`이다.

### Grant or change access

```http
PUT /api/v1/workspaces/{workspace_id}/access/{account_id}
```

```json
{
  "role": "viewer"
}
```

대상 account가 active 상태일 때 access를 생성하거나 role을 변경한다. 이미 revoke된 row가 있으면 같은 `(workspace_id, account_id)` row를 다시 활성화한다.

### Revoke access

```http
DELETE /api/v1/workspaces/{workspace_id}/access/{account_id}
```

대상 access에 `revoked_at`/`revoked_by`를 설정한다. 현재 상태 attribution field는 그대로 유지한다. 마지막 live `owner`를 revoke하는 요청은 거부한다. 이미 live grant가 없는 account를 revoke하는 요청은 caller owner check 후 성공으로 처리한다.
