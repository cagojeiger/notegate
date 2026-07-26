import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { Space } from "../api/types";
import type { SpaceUsage } from "../api/usage";
import { StatusBar } from "./StatusBar";

describe("StatusBar", () => {
  it("shows the save state and active space", () => {
    render(<StatusBar activeSpace={null} />);

    expect(screen.getByText("ready")).toBeInTheDocument();
    expect(screen.getByText("No space")).toBeInTheDocument();
  });

  it("shows active space usage and new-item defaults", () => {
    render(<StatusBar activeSpace={space} usage={usage} />);

    expect(screen.getByText("319 items")).toBeInTheDocument();
    expect(screen.getByText("3 MB used")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "New items are included in search" })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "New text encryption is off" })).toBeInTheDocument();
    expect(screen.getByText("Personal")).toBeInTheDocument();
  });
});

const space: Space = {
  id: "space-1",
  name: "Personal",
  sort_order: 0,
  navigation_pinned: true,
  user_mcp_enabled: true,
  default_search_enabled: true,
  default_text_encryption_enabled: false,
  features: { text_encryption: true },
  permission: "write",
  root_node_id: "root-1",
  created_at: "2026-07-01T00:00:00Z",
  updated_at: "2026-07-01T00:00:00Z"
};

const usage: SpaceUsage = {
  id: space.id,
  name: space.name,
  items: { used: 319, limit: 1_999 },
  text_bytes: { used: 1024 * 1024, limit: 128 * 1024 * 1024 },
  file_bytes: { used: 2 * 1024 * 1024, limit: 100 * 1024 * 1024 * 1024 },
  reconciliation_pending: false
};
