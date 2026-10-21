ALTER TABLE oauth_credentials ADD COLUMN private_refresh_response JSONB;
ALTER TABLE oauth_credentials ADD CONSTRAINT oauth_refresh_response_consumed
    CHECK (private_refresh_response IS NULL OR (kind = 'refresh' AND consumed_at IS NOT NULL));

CREATE INDEX oauth_refresh_response_cleanup_idx ON oauth_credentials(consumed_at, token_hash)
    WHERE private_refresh_response IS NOT NULL;
