-- Reuse encrypted invocation payloads, owner/surface pagination, and retention.
ALTER TABLE command_invocations
  DROP CONSTRAINT command_invocations_surface_valid,
  ADD CONSTRAINT command_invocations_surface_valid CHECK (
    surface IN ('mcp', 'cli', 'api')
  );
