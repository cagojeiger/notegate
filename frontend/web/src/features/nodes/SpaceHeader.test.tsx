import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { Space } from "../../api/types";
import { SpaceHeader } from "./SpaceHeader";

vi.mock("./useNodeQueries", () => ({
  useRefreshSpace: () => vi.fn()
}));

const activeSpace: Space = {
  id: "space-1",
  name: "Meetings",
  sort_order: 0,
  navigation_pinned: false,
  user_mcp_enabled: false,
  default_search_enabled: false,
  default_text_encryption_enabled: false,
  features: { text_encryption: false, write_lock: false },
  permission: "write",
  root_node_id: "root-1",
  created_at: "2026-08-11T00:00:00Z",
  updated_at: "2026-08-11T00:00:00Z"
};

describe("SpaceHeader", () => {
  it("opens the create menu and starts audio recording", async () => {
    const user = userEvent.setup();
    const onRecordAudio = vi.fn();

    render(
      <SpaceHeader
        activeSpace={activeSpace}
        canWriteActiveSpace
        canManageActiveSpace
        onCreateFolder={vi.fn()}
        onCreateText={vi.fn()}
        onRecordAudio={onRecordAudio}
        onFileSelected={vi.fn()}
        onRenameSpace={vi.fn()}
        onDeleteSpace={vi.fn()}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Create" }));
    await user.click(screen.getByRole("button", { name: "Record audio" }));

    expect(onRecordAudio).toHaveBeenCalledOnce();
    expect(screen.queryByRole("button", { name: "Record audio" })).not.toBeInTheDocument();
  });
});
