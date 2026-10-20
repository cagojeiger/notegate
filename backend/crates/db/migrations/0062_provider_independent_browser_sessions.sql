LOCK TABLE browser_sessions IN ACCESS EXCLUSIVE MODE;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM browser_sessions s
        WHERE NOT EXISTS (
            SELECT 1 FROM audit_events a
            WHERE a.op_type = 'session.migrate'
              AND a.source = 'system'
              AND a.actor_account_id IS NULL
              AND a.owner_user_id = s.user_id
              AND a.resource_type = 'browser_session'
              AND a.resource_id = s.id
              AND a.metadata->>'reason' = 'provider_independent'
              AND length(a.metadata->>'trusted_issuer') > 0
              AND a.metadata->'issuer' = coalesce(to_jsonb(s.oidc_issuer), 'null'::jsonb)
              AND (s.revoked_at IS NOT NULL
                   OR s.oidc_issuer = a.metadata->>'trusted_issuer')
        )
    ) THEN
        RAISE EXCEPTION 'Stop API/workers and run --prepare-browser-sessions with the previous OIDC configuration before migration 62';
    END IF;
END $$;

ALTER TABLE browser_sessions DROP COLUMN oidc_issuer;
