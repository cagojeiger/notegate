import type { AccountRef, NodeSummary, RestNode, Space } from "../api/types";

const timestamp = "2026-07-01T00:00:00Z";

function makeAccountRef(overrides: Partial<AccountRef> = {}): AccountRef {
  return {
    id: "user-1",
    kind: "user",
    display_name: "User",
    ...overrides
  };
}

export function makeSpace(overrides: Partial<Space> = {}): Space {
  return {
    id: "space-1",
    name: "Daily",
    sort_order: 0,
    navigation_pinned: true,
    user_mcp_enabled: true,
    default_search_enabled: true,
    default_text_encryption_enabled: false,
    permission: "write",
    root_node_id: "root-1",
    created_at: timestamp,
    updated_at: timestamp,
    ...overrides,
    features: {
      text_encryption: true,
      write_lock: true,
      ...overrides.features
    }
  };
}

export function makeNodeSummary(overrides: Partial<NodeSummary> = {}): NodeSummary {
  return {
    id: "node-1",
    space_id: "space-1",
    parent_id: "root-1",
    name: "note.md",
    kind: "text",
    path: "/note.md",
    has_children: false,
    effective_write_locked: false,
    updated_at: timestamp,
    ...overrides
  };
}

export function makeRestNode(overrides: Partial<RestNode> = {}): RestNode {
  return {
    ...makeNodeSummary(overrides),
    sort_order: 0,
    metadata: {},
    search_enabled: true,
    write_locked: false,
    write_lock_sources: [],
    created_by: makeAccountRef(),
    updated_by: makeAccountRef(),
    created_at: timestamp,
    ...overrides
  };
}
