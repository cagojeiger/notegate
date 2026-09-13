ALTER TABLE nodes RENAME COLUMN search_enabled TO external_access_enabled;
ALTER TABLE spaces RENAME COLUMN default_search_enabled TO default_external_access_enabled;

-- Keep direct settings intact; external access requires the entire ancestor chain.
-- Deleted rows remain eligible for deletion-history authorization until purged.
CREATE FUNCTION node_external_access_allowed(target_space_id uuid, target_node_id uuid)
RETURNS boolean
LANGUAGE sql STABLE
AS $$
    WITH RECURSIVE ancestors AS (
        SELECT id, parent_id, external_access_enabled
        FROM nodes WHERE space_id = target_space_id AND id = target_node_id
        UNION
        SELECT n.id, n.parent_id, n.external_access_enabled
        FROM nodes n JOIN ancestors a ON n.id = a.parent_id
        WHERE n.space_id = target_space_id
    )
    SELECT COALESCE(bool_and(external_access_enabled) AND bool_or(parent_id IS NULL), false)
    FROM ancestors
$$;
