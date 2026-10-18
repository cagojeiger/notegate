-- Verify every retained legacy identity before removing its compatibility data.
-- Deleted accounts retain their identity tombstones until the normal purge.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM users u
        WHERE (u.anonymized_at IS NULL AND NOT EXISTS (
                  SELECT 1 FROM external_identities i WHERE i.user_id = u.id
              ))
           OR (u.provider_sub_hash IS NOT NULL AND NOT EXISTS (
              SELECT 1 FROM external_identities i
              JOIN legacy_identity_issuer b ON b.issuer = i.issuer
              WHERE i.user_id = u.id
          ))
    ) THEN
        RAISE EXCEPTION 'Link all retained accounts to verified external identities before migration 58';
    END IF;
END $$;

ALTER TABLE users
    DROP COLUMN provider_sub_hash,
    DROP COLUMN provider_sub_hash_key_id,
    DROP COLUMN provider_sub_hash_version;

DROP TABLE legacy_identity_issuer;
