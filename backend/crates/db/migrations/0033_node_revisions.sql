ALTER TABLE nodes
    ADD COLUMN revision BIGINT NOT NULL DEFAULT 1
    CHECK (revision > 0);

CREATE OR REPLACE FUNCTION increment_node_revision()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    NEW.revision := OLD.revision + 1;
    RETURN NEW;
END;
$$;

CREATE TRIGGER nodes_increment_revision
BEFORE UPDATE ON nodes
FOR EACH ROW
EXECUTE FUNCTION increment_node_revision();
