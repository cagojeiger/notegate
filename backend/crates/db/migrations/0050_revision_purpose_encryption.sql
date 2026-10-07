ALTER TABLE text_objects ADD COLUMN revision_private_purpose JSONB,
  ADD CONSTRAINT text_objects_purpose_encrypted CHECK (
    revision_purpose IS NULL OR revision_private_purpose IS NULL
  );
ALTER TABLE text_revisions ADD COLUMN private_purpose JSONB,
  ADD CONSTRAINT text_revisions_purpose_encrypted CHECK (
    purpose IS NULL OR private_purpose IS NULL
  );
CREATE INDEX text_objects_plain_purpose_idx ON text_objects(node_id)
  WHERE revision_purpose IS NOT NULL;
CREATE INDEX text_revisions_plain_purpose_idx ON text_revisions(id)
  WHERE purpose IS NOT NULL;

-- During rolling deployment an old writer copies only the plaintext column.
-- Preserve the encrypted old-head reason when it creates that exact revision.
CREATE FUNCTION preserve_revision_private_purpose() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF NEW.purpose IS NULL AND NEW.private_purpose IS NULL THEN
    SELECT revision_private_purpose INTO NEW.private_purpose FROM text_objects
      WHERE space_id=NEW.space_id AND node_id=NEW.node_id AND revision_id=NEW.id;
  END IF;
  RETURN NEW;
END;
$$;
CREATE TRIGGER text_revisions_preserve_private_purpose BEFORE INSERT ON text_revisions
  FOR EACH ROW EXECUTE FUNCTION preserve_revision_private_purpose();

-- An old writer cannot carry a previous body's encrypted reason onto a new ID.
CREATE FUNCTION clear_replaced_private_purpose() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF NEW.revision_id IS DISTINCT FROM OLD.revision_id
     AND NEW.revision_private_purpose IS NOT DISTINCT FROM OLD.revision_private_purpose THEN
    NEW.revision_private_purpose := NULL;
  END IF;
  RETURN NEW;
END;
$$;
CREATE TRIGGER text_objects_clear_replaced_private_purpose BEFORE UPDATE ON text_objects
  FOR EACH ROW EXECUTE FUNCTION clear_replaced_private_purpose();
