# REST Access

## Access

Owner-only APIs for granting users or agents access to a workspace. A workspace has at most `20` active access accounts.

### List access

```http
GET /api/v1/workspaces/{workspace_id}/access?limit=100&cursor=...
```

Default and max limit are `100`.

### Grant or change access

```http
PUT /api/v1/workspaces/{workspace_id}/access/{account_id}
```

```json
{
  "role": "viewer"
}
```

### Revoke access

```http
DELETE /api/v1/workspaces/{workspace_id}/access/{account_id}
```

Revokes access by setting `revoked_at`/`revoked_by`; current-state attribution fields remain valid.
