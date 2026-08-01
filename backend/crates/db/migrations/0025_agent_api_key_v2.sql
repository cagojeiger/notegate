-- Retire the legacy API-key credential format. Existing rows remain for
-- audit history but no longer count as live credentials.

UPDATE api_keys
SET revoked_at = now(),
    revoked_reason = 'credential_format_v1_retired'
WHERE revoked_at IS NULL
  AND left(token_prefix, length('ngk_v1_')) = 'ngk_v1_';
