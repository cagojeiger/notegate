-- User API keys are no longer an authentication surface. Retire their live
-- credentials while preserving Agent keys for the staged v2 rollout.
WITH retired AS (
  UPDATE api_keys AS k
  SET revoked_at = now(),
      revoked_reason = 'user_api_key_retired'
  FROM accounts AS a
  WHERE k.account_id = a.id
    AND a.kind = 'user'
    AND k.revoked_at IS NULL
    AND k.expires_at > now()
  RETURNING k.id, k.account_id
)
INSERT INTO audit_events (
  owner_user_id,
  actor_account_id,
  source,
  op_type,
  resource_type,
  resource_id,
  metadata
)
SELECT
  account_id,
  NULL,
  'system',
  'user_key.revoke',
  'api_key',
  id,
  jsonb_build_object('reason', 'user_api_key_retired')
FROM retired;
