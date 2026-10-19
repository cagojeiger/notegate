ALTER TABLE browser_sessions RENAME COLUMN validated_until TO idle_expires_at;
ALTER TABLE browser_sessions ADD COLUMN client_environment TEXT;
ALTER TABLE oauth_grants ADD COLUMN client_environment TEXT;
ALTER TABLE oauth_grants ADD COLUMN last_token_at TIMESTAMPTZ;
