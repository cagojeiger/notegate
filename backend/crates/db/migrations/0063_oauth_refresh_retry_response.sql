ALTER TABLE oauth_credentials ADD COLUMN private_refresh_response JSONB;
ALTER TABLE oauth_credentials ADD COLUMN refresh_retry_until TIMESTAMPTZ;
ALTER TABLE oauth_credentials ADD CONSTRAINT oauth_refresh_response_consumed
    CHECK (
        (private_refresh_response IS NULL AND refresh_retry_until IS NULL)
        OR (private_refresh_response IS NOT NULL AND refresh_retry_until IS NOT NULL
            AND kind = 'refresh' AND consumed_at IS NOT NULL)
    );

CREATE INDEX oauth_refresh_response_cleanup_idx ON oauth_credentials(refresh_retry_until, token_hash)
    WHERE private_refresh_response IS NOT NULL;
