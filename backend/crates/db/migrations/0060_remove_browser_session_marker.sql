ALTER TABLE browser_sessions
    DROP CONSTRAINT browser_session_issuer,
    DROP COLUMN local_session;
