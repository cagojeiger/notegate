ALTER TABLE mcp_invocations
  ADD COLUMN space_name TEXT;

ALTER TABLE mcp_invocations
  ADD CONSTRAINT mcp_invocations_space_name_valid CHECK (
    space_name IS NULL
    OR (
      char_length(space_name) BETWEEN 1 AND 63
      AND space_name = btrim(space_name)
      AND space_name NOT IN ('.', '..')
      AND space_name NOT LIKE '%/%'
      AND space_name NOT LIKE '%:%'
      AND space_name !~ '[[:cntrl:]]'
    )
  );

ALTER TABLE mcp_invocations
  ADD CONSTRAINT mcp_invocations_space_name_scope CHECK (
    space_name IS NULL OR (tool = 'read' AND op = 'changes')
  );
