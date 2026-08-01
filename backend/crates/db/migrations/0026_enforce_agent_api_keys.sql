CREATE OR REPLACE FUNCTION enforce_agent_api_key_owner()
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

  RETURN NEW;
END;
$$;

CREATE TRIGGER api_keys_agent_owner
BEFORE INSERT OR UPDATE OF account_id ON api_keys
FOR EACH ROW
EXECUTE FUNCTION enforce_agent_api_key_owner();

-- Re-run retirement after installing the write guard. Concurrent inserts either
-- commit before this update and are retired, or wait for the migration and are
-- rejected by the trigger.
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
