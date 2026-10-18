-- Upstream refresh credentials are no longer part of NoteGate browser sessions.
-- Stop older API processes before applying: they still SELECT/INSERT these columns.
ALTER TABLE browser_sessions
    DROP COLUMN refresh_token_ciphertext,
    DROP COLUMN refresh_token_nonce,
    DROP COLUMN refresh_token_enc_key_id,
    DROP COLUMN refresh_token_enc_version,
    DROP COLUMN last_refreshed_at,
    DROP COLUMN refresh_started_at,
    DROP COLUMN refresh_claim_id;
