-- Per-Space defaults, per-node search visibility, and server-managed text encryption.

ALTER TABLE spaces
    ADD COLUMN default_search_enabled BOOLEAN NOT NULL DEFAULT true,
    ADD COLUMN default_text_encryption_enabled BOOLEAN NOT NULL DEFAULT false;

ALTER TABLE nodes
    ADD COLUMN search_enabled BOOLEAN NOT NULL DEFAULT true;

ALTER TABLE text_objects
    ADD COLUMN encryption_enabled BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN at_rest_encryption TEXT NOT NULL DEFAULT 'none'
        CHECK (at_rest_encryption IN ('none', 'server')),
    ADD COLUMN content_ciphertext BYTEA,
    ADD COLUMN content_nonce BYTEA,
    ADD COLUMN content_enc_key_id TEXT REFERENCES crypto_key_epochs(key_id),
    ADD COLUMN content_enc_version INTEGER;

ALTER TABLE text_objects
    DROP CONSTRAINT text_objects_check,
    ADD CONSTRAINT text_objects_storage_check CHECK (
        (
            storage_format = 'plain'
            AND encrypted_payload IS NULL
            AND (
                (
                    at_rest_encryption = 'none'
                    AND content_text IS NOT NULL
                    AND content_ciphertext IS NULL
                    AND content_nonce IS NULL
                    AND content_enc_key_id IS NULL
                    AND content_enc_version IS NULL
                )
                OR
                (
                    at_rest_encryption = 'server'
                    AND content_text IS NULL
                    AND content_ciphertext IS NOT NULL
                    AND content_nonce IS NOT NULL
                    AND content_enc_key_id IS NOT NULL
                    AND content_enc_version IS NOT NULL
                )
            )
        )
        OR
        (
            storage_format = 'encrypted'
            AND at_rest_encryption = 'none'
            AND content_text IS NULL
            AND encrypted_payload IS NOT NULL
            AND jsonb_typeof(encrypted_payload) = 'object'
            AND content_ciphertext IS NULL
            AND content_nonce IS NULL
            AND content_enc_key_id IS NULL
            AND content_enc_version IS NULL
        )
    );
