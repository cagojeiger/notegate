DROP TRIGGER IF EXISTS api_keys_agent_owner ON api_keys;
DROP FUNCTION IF EXISTS enforce_agent_api_key_owner();

CREATE FUNCTION enforce_v2_agent_api_key()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
  IF NOT EXISTS (
    SELECT 1
    FROM accounts
    WHERE id = NEW.account_id
      AND kind = 'agent'
  ) THEN
    RAISE EXCEPTION 'api keys may only belong to agent accounts'
      USING ERRCODE = '23514',
            CONSTRAINT = 'api_keys_agent_owner';
  END IF;

  IF NEW.token_prefix <> ('ngk_v2_' || NEW.id::text) THEN
    RAISE EXCEPTION 'agent api key prefix must match ngk_v2_{key_id}'
      USING ERRCODE = '23514',
            CONSTRAINT = 'api_keys_v2_prefix';
  END IF;

  RETURN NEW;
END;
$$;

CREATE TRIGGER api_keys_v2_agent_owner
BEFORE INSERT OR UPDATE OF account_id, token_prefix ON api_keys
FOR EACH ROW
EXECUTE FUNCTION enforce_v2_agent_api_key();

-- The DDL lock closes the rolling-deployment race: old writes either finish
-- before this retirement pass or wait and are rejected by the new trigger.
WITH retired AS (
  UPDATE api_keys AS k
  SET revoked_at = now(),
      revoked_reason = CASE
        WHEN a.kind = 'user' THEN 'user_api_key_retired'
        WHEN left(k.token_prefix, 7) = 'ngk_v1_' THEN 'legacy_api_key_retired'
        ELSE 'invalid_api_key_format_retired'
      END
  FROM accounts AS a
  WHERE k.account_id = a.id
    AND k.revoked_at IS NULL
    AND k.expires_at > now()
    AND (
      a.kind <> 'agent'
      OR k.token_prefix <> ('ngk_v2_' || k.id::text)
    )
  RETURNING k.id, k.account_id, a.kind, k.revoked_reason
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
  CASE
    WHEN retired.kind = 'user' THEN retired.account_id
    ELSE agents.owner_user_id
  END,
  NULL,
  'system',
  CASE
    WHEN retired.kind = 'user' THEN 'user_key.revoke'
    ELSE 'agent_key.revoke'
  END,
  'api_key',
  retired.id,
  jsonb_build_object('reason', retired.revoked_reason)
FROM retired
LEFT JOIN agents ON agents.id = retired.account_id;
