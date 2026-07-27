-- Direct node locks. Descendants inherit the effective write barrier through
-- their live parent chain; inherited state is intentionally not materialized.

ALTER TABLE nodes
    ADD COLUMN write_locked BOOLEAN NOT NULL DEFAULT false;

ALTER TABLE nodes
    ADD CONSTRAINT nodes_root_cannot_be_write_locked
    CHECK (parent_id IS NOT NULL OR write_locked = false);
