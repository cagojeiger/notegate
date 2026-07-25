# ADR 0006: User MCP pinned Space scope

## Context

Owner users need to keep some Spaces in the dashboard without exposing them through their user-authenticated MCP. Agent access already has a separate explicit connection model.

## Decision

Add an owner-controlled Pin state to each Space.

- REST/dashboard lists every owned live Space.
- User MCP lists and accesses only Pinned Spaces.
- Agent MCP ignores Pin and accesses only explicitly connected Spaces.
- Unpinned and nonexistent Spaces return the same MCP not-found behavior.
- New Spaces start Unpinned.
- Existing live Spaces are backfilled Pinned during migration to preserve current user MCP access.

Pin is an authorization boundary for user MCP, not only a navigation preference. It applies to all MCP target resolution and transfer continuation operations.

## Consequences

- The Space Library is the owner management surface for Pinned and Unpinned Spaces.
- The user Workbench rail and mobile switcher show only Pinned Spaces.
- An owner may still open an Unpinned Space from the Library.
- Agent connection permission remains the only Agent Space authorization rule.
- Unpin blocks subsequent MCP operations immediately, but does not revoke a
  presigned transfer URL that was already issued. That URL remains usable until
  its existing short expiry.
- Collections and additional Space policy controls remain outside this decision.
