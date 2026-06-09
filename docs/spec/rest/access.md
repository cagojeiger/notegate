# REST Access

## Access

Access API는 workspace lifecycle owner-only endpoint다. Lifecycle owner는 `workspaces.created_by` user에서 derive하며, access row에는 저장하지 않는다. Owner는 active user/agent account에 `viewer/editor` grant를 부여하거나 회수할 수 있다. 한 workspace는 implicit owner를 제외하고 최대 `20`개의 active granted account를 가진다.

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

대상 account가 active 상태일 때 `viewer` 또는 `editor` access를 생성하거나 role을 변경한다. `owner` role은 입력으로 받지 않는다. 이미 revoke된 row가 있으면 같은 `(workspace_id, account_id)` row를 다시 활성화한다. 이때 현재 grant 상태의 `granted_by`/`granted_at`을 갱신한다. implicit owner를 제외한 active granted account가 `20`개를 넘으면 `409 conflict`로 거부한다.

### Revoke access

```http
DELETE /api/v1/workspaces/{workspace_id}/access/{account_id}
```

대상 access에 `revoked_at`/`revoked_by`를 설정한다. 현재 grant attribution field는 그대로 유지한다. Lifecycle owner는 access row가 아니므로 revoke 대상이 아니다. 이미 live grant가 없는 account를 revoke하는 요청은 caller owner check 후 성공으로 처리한다.
