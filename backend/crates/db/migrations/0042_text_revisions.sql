-- Body attribution is independent from metadata/encryption changes.
ALTER TABLE text_objects
    ADD COLUMN revision_id UUID NOT NULL DEFAULT gen_random_uuid(),
    ADD COLUMN revision_written_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN revision_author_id UUID REFERENCES accounts(id),
    ADD COLUMN revision_group_id UUID NOT NULL DEFAULT gen_random_uuid(),
    ADD COLUMN revision_group_started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN revision_session_id UUID,
    ADD COLUMN revision_source TEXT NOT NULL DEFAULT 'unknown';
UPDATE text_objects SET revision_written_at = updated_at,
    revision_group_started_at = updated_at, revision_author_id = updated_by_account_id;
ALTER TABLE text_objects ALTER COLUMN revision_author_id SET NOT NULL;
CREATE FUNCTION initialize_text_revision() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    NEW.revision_author_id := NEW.updated_by_account_id;
    RETURN NEW;
END;
$$;
CREATE TRIGGER initialize_text_revision BEFORE INSERT ON text_objects
    FOR EACH ROW EXECUTE FUNCTION initialize_text_revision();

CREATE TABLE text_revisions (
    id UUID PRIMARY KEY,
    node_id UUID NOT NULL,
    space_id UUID NOT NULL,
    content_sha256 TEXT NOT NULL,
    byte_len BIGINT NOT NULL CHECK (byte_len BETWEEN 0 AND 1048576),
    line_count INTEGER NOT NULL,
    written_at TIMESTAMPTZ NOT NULL,
    author_id UUID NOT NULL REFERENCES accounts(id),
    group_id UUID NOT NULL,
    source TEXT NOT NULL,
    checkpoint BOOLEAN NOT NULL,
    superseded_at TIMESTAMPTZ NOT NULL,
    -- Precomputed once; replaying cleanup cannot change grouping boundaries.
    cleanup_at TIMESTAMPTZ NOT NULL,
    ciphertext BYTEA NOT NULL,
    nonce BYTEA NOT NULL,
    enc_key_id TEXT NOT NULL,
    enc_version INTEGER NOT NULL,
    stored_bytes BIGINT GENERATED ALWAYS AS (octet_length(ciphertext)::bigint + octet_length(nonce)) STORED,
    FOREIGN KEY (node_id, space_id) REFERENCES text_objects(node_id, space_id) ON DELETE CASCADE
);
CREATE INDEX text_revisions_list_idx ON text_revisions(space_id, node_id, superseded_at DESC, id DESC);
CREATE INDEX text_revisions_cleanup_idx ON text_revisions(cleanup_at, id);
CREATE INDEX text_revisions_space_cleanup_idx ON text_revisions(space_id, cleanup_at, id);

-- Kept separate from live-content usage, including while a document is soft-deleted.
CREATE TABLE text_revision_usage (
    space_id UUID PRIMARY KEY REFERENCES spaces(id) ON DELETE CASCADE,
    stored_bytes BIGINT NOT NULL DEFAULT 0 CHECK (stored_bytes >= 0)
);
CREATE FUNCTION release_text_revision_usage() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    UPDATE text_revision_usage SET stored_bytes = stored_bytes - OLD.stored_bytes
        WHERE space_id = OLD.space_id;
    RETURN OLD;
END;
$$;
CREATE TRIGGER release_text_revision_usage AFTER DELETE ON text_revisions
    FOR EACH ROW EXECUTE FUNCTION release_text_revision_usage();
