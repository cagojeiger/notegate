-- Cluster-singleton retention scans terminal ledger rows by their completion time.
CREATE INDEX object_storage_objects_terminal_retention_idx
    ON object_storage_objects (
        (COALESCE(deleted_at, last_activity_at)),
        id
    )
    WHERE state IN ('expired', 'deleted');
