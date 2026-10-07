-- Content quota follows retained bytes, including trash and pending S3 deletion.
-- The accounting scope outlives the Space; no names or content are copied here.
LOCK TABLE spaces, text_objects IN SHARE ROW EXCLUSIVE MODE;

CREATE TABLE space_storage_usage (
    space_id UUID PRIMARY KEY,
    owner_user_id UUID NOT NULL,
    text_bytes BIGINT NOT NULL DEFAULT 0 CHECK (text_bytes >= 0),
    file_bytes BIGINT NOT NULL DEFAULT 0 CHECK (file_bytes >= 0)
);
CREATE INDEX space_storage_usage_owner_idx ON space_storage_usage(owner_user_id, space_id);

ALTER TABLE object_storage_objects ADD COLUMN usage_space_id UUID;
UPDATE object_storage_objects SET usage_space_id = space_id;
CREATE INDEX object_storage_objects_usage_idx ON object_storage_objects(usage_space_id, state)
    WHERE usage_space_id IS NOT NULL;

INSERT INTO space_storage_usage(space_id, owner_user_id, text_bytes, file_bytes)
SELECT s.id, s.owner_user_id,
    COALESCE((SELECT sum(t.byte_len) FROM text_objects t WHERE t.space_id = s.id), 0),
    COALESCE((SELECT sum(o.declared_byte_len) FROM object_storage_objects o
        WHERE o.usage_space_id = s.id AND o.state IN ('attached', 'delete_pending')), 0)
FROM spaces s;

CREATE FUNCTION create_space_storage_usage() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO space_storage_usage(space_id, owner_user_id) VALUES (NEW.id, NEW.owner_user_id);
    RETURN NEW;
END;
$$;
CREATE TRIGGER spaces_create_storage_usage AFTER INSERT ON spaces
    FOR EACH ROW EXECUTE FUNCTION create_space_storage_usage();

CREATE FUNCTION preserve_object_usage_space() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'UPDATE' AND OLD.usage_space_id IS NOT NULL THEN
        NEW.usage_space_id := OLD.usage_space_id;
    ELSE
        NEW.usage_space_id := NEW.space_id;
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER object_storage_usage_scope BEFORE INSERT OR UPDATE OF space_id, usage_space_id
    ON object_storage_objects FOR EACH ROW EXECUTE FUNCTION preserve_object_usage_space();

CREATE FUNCTION account_stored_text_bytes() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    scope_id UUID;
    delta BIGINT;
BEGIN
    IF TG_OP = 'DELETE' THEN
        scope_id := OLD.space_id;
        delta := -OLD.byte_len;
    ELSIF TG_OP = 'INSERT' THEN
        scope_id := NEW.space_id;
        delta := NEW.byte_len;
    ELSE
        scope_id := NEW.space_id;
        delta := NEW.byte_len - OLD.byte_len;
    END IF;
    IF delta <> 0 THEN
        UPDATE space_storage_usage SET text_bytes = text_bytes + delta WHERE space_id = scope_id;
        IF NOT FOUND THEN RAISE EXCEPTION 'missing retained storage usage'; END IF;
    END IF;
    RETURN NULL;
END;
$$;
CREATE TRIGGER text_objects_storage_usage AFTER INSERT OR UPDATE OF byte_len OR DELETE
    ON text_objects FOR EACH ROW EXECUTE FUNCTION account_stored_text_bytes();

CREATE FUNCTION account_stored_file_bytes() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    scope_id UUID;
    before_bytes BIGINT := 0;
    after_bytes BIGINT := 0;
BEGIN
    IF TG_OP <> 'INSERT' THEN
        scope_id := OLD.usage_space_id;
        IF OLD.state IN ('attached', 'delete_pending') THEN before_bytes := OLD.declared_byte_len; END IF;
    END IF;
    IF TG_OP <> 'DELETE' THEN
        scope_id := NEW.usage_space_id;
        IF NEW.state IN ('attached', 'delete_pending') THEN after_bytes := NEW.declared_byte_len; END IF;
    END IF;
    -- Legacy objects already detached before this migration have unknown scope.
    IF scope_id IS NOT NULL AND after_bytes <> before_bytes THEN
        UPDATE space_storage_usage SET file_bytes = file_bytes + after_bytes - before_bytes
            WHERE space_id = scope_id;
        IF NOT FOUND THEN RAISE EXCEPTION 'missing retained storage usage'; END IF;
    END IF;
    RETURN NULL;
END;
$$;
CREATE TRIGGER object_storage_retained_usage AFTER INSERT OR UPDATE OF state, declared_byte_len OR DELETE
    ON object_storage_objects FOR EACH ROW EXECUTE FUNCTION account_stored_file_bytes();
