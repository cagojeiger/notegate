import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { Me, RestNode, Space } from "../api/types";
import { AppShell } from "./AppShell";

const mocks = vi.hoisted(() => ({
  useWorkbenchController: vi.fn()
}));

vi.mock("../features/workbench/useWorkbenchController", () => ({
  useWorkbenchController: mocks.useWorkbenchController
}));

vi.mock("../features/editor/EditorArea", () => ({ EditorArea: () => null }));
vi.mock("../features/nodes/PrimarySidebar", () => ({ PrimarySidebar: () => null }));
vi.mock("../features/spaces/MobileSpaceBar", () => ({ MobileSpaceBar: () => null }));
vi.mock("./AuxiliarySidebar", () => ({ AuxiliarySidebar: () => null }));
vi.mock("../features/events/EventHistoryModal", () => ({
  EventHistoryModal: ({ activeSpace, activeNode, canViewAuditEvents }: { activeSpace: Space | null; activeNode: RestNode | null; canViewAuditEvents: boolean }) => (
    <div
      data-testid="history-modal"
      data-space-id={activeSpace?.id}
      data-node-id={activeNode?.id}
      data-can-view-audit={String(canViewAuditEvents)}
    />
  )
}));

const space: Space = {
  id: "space-1",
  name: "Daily",
  sort_order: 0,
  permission: "write",
  root_node_id: "root-1",
  created_at: "2026-07-01T00:00:00Z",
  updated_at: "2026-07-10T00:00:00Z"
};

const activeNode: RestNode = {
  id: "node-1",
  space_id: space.id,
  parent_id: space.root_node_id,
  name: "note.md",
  kind: "text",
  path: "/note.md",
  sort_order: 0,
  metadata: {},
  has_children: false,
  created_by: { id: "user-1", kind: "user", display_name: "User" },
  updated_by: { id: "user-1", kind: "user", display_name: "User" },
  created_at: "2026-07-10T02:00:00Z",
  updated_at: "2026-07-10T02:12:00Z"
};

describe("AppShell history", () => {
  it.each([
    ["user", true],
    ["agent", false]
  ] as const)("opens the current scope for a %s account", async (kind, canViewAudit) => {
    const user = userEvent.setup();
    mocks.useWorkbenchController.mockReturnValue(workbench());

    render(<AppShell me={me(kind)} onSignOut={vi.fn()} />);

    await user.click(screen.getByRole("button", { name: "History" }));

    const modal = screen.getByTestId("history-modal");
    expect(modal).toHaveAttribute("data-space-id", space.id);
    expect(modal).toHaveAttribute("data-node-id", activeNode.id);
    expect(modal).toHaveAttribute("data-can-view-audit", String(canViewAudit));
  });
});

function me(kind: Me["account"]["kind"]): Me {
  return {
    account: { id: `${kind}-1`, kind, display_name: kind },
    capabilities: { can_create_space: kind === "user", can_manage_agents: kind === "user" }
  };
}

function workbench() {
  return {
    loading: false,
    error: null,
    spaces: [space],
    theme: "light",
    activeSpace: space,
    activeNode,
    canCreateSpace: true,
    canWriteActiveSpace: true,
    canManageActiveSpace: true,
    editorGroups: [],
    activeGroupIndex: 0,
    expandedFolderIds: new Set<string>(),
    primarySidebarOpen: true,
    auxiliaryOpen: false,
    primaryWidth: 300,
    mobileTreeOpen: false,
    mobileAuxOpen: false,
    showAuxiliary: false,
    isMobile: false,
    settingsOpen: false,
    dialog: null,
    actions: {}
  };
}
