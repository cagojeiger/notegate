import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { Me, RestNode, Space } from "../api/types";
import { AppShell } from "./AppShell";

const mocks = vi.hoisted(() => ({
  useWorkbenchController: vi.fn(),
  useUploadManager: vi.fn()
}));

vi.mock("../features/workbench/useWorkbenchController", () => ({
  useWorkbenchController: mocks.useWorkbenchController
}));

vi.mock("../features/uploads/UploadProvider", () => ({
  useUploadManager: mocks.useUploadManager
}));

vi.mock("../features/editor/EditorArea", () => ({ EditorArea: () => null }));
vi.mock("../features/nodes/PrimarySidebar", () => ({ PrimarySidebar: () => null }));
vi.mock("../features/spaces/MobileSpaceBar", () => ({ MobileSpaceBar: () => null }));
vi.mock("../features/spaces/SpaceLibrary", () => ({
  SpaceLibrary: ({ spaces }: { spaces: Space[] }) => <div data-testid="space-library">{spaces.map((item) => item.name).join(",")}</div>
}));
vi.mock("./AuxiliarySidebar", () => ({ AuxiliarySidebar: () => null }));
vi.mock("../features/events/EventHistoryModal", () => ({
  EventHistoryModal: ({ spaces, initialSpaceId, canViewAuditEvents }: { spaces: Space[]; initialSpaceId: string | null; canViewAuditEvents: boolean }) => (
    <div
      data-testid="history-modal"
      data-space-id={initialSpaceId ?? undefined}
      data-space-count={spaces.length}
      data-can-view-audit={String(canViewAuditEvents)}
    />
  )
}));

const space: Space = {
  id: "space-1",
  name: "Daily",
  sort_order: 0,
  navigation_pinned: true,
  user_mcp_enabled: true,
  default_search_enabled: true,
  default_text_encryption_enabled: false,
  features: { text_encryption: true },
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

const privateSpace: Space = {
  ...space,
  id: "space-2",
  name: "Private",
  navigation_pinned: false,
  user_mcp_enabled: false,
  root_node_id: "root-2"
};

describe("AppShell history", () => {
  it.each([
    ["user", true],
    ["agent", false]
  ] as const)("opens the current scope for a %s account", async (kind, canViewAudit) => {
    const user = userEvent.setup();
    mocks.useWorkbenchController.mockReturnValue(workbench());
    mocks.useUploadManager.mockReturnValue(uploadManager());

    render(<AppShell me={me(kind)} onSignOut={vi.fn()} />);

    await user.click(screen.getByRole("button", { name: "History" }));

    const modal = screen.getByTestId("history-modal");
    expect(modal).toHaveAttribute("data-space-id", space.id);
    expect(modal).toHaveAttribute("data-space-count", "1");
    expect(modal).toHaveAttribute("data-can-view-audit", String(canViewAudit));
  });

  it("keeps inactive navigation-unpinned spaces out of the user rail while showing all spaces in the library", async () => {
    const user = userEvent.setup();
    mocks.useWorkbenchController.mockReturnValue({ ...workbench(), spaces: [space, privateSpace] });
    mocks.useUploadManager.mockReturnValue(uploadManager());

    render(<AppShell me={me("user")} onSignOut={vi.fn()} />);

    expect(screen.getByRole("button", { name: "Daily" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Private" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Open space library" }));

    expect(screen.getByTestId("space-library")).toHaveTextContent("Daily,Private");
    expect(screen.getByText("ready")).toBeInTheDocument();
  });

  it("keeps the active navigation-unpinned space out of the user rail", () => {
    mocks.useWorkbenchController.mockReturnValue({
      ...workbench(),
      spaces: [space, privateSpace],
      activeSpace: privateSpace
    });
    mocks.useUploadManager.mockReturnValue(uploadManager());

    render(<AppShell me={me("user")} onSignOut={vi.fn()} />);

    expect(screen.getByRole("button", { name: "Daily" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Private" })).not.toBeInTheDocument();
  });

  it("keeps connected spaces in the agent rail and does not expose the pin library", () => {
    mocks.useWorkbenchController.mockReturnValue({ ...workbench(), spaces: [space, privateSpace] });
    mocks.useUploadManager.mockReturnValue(uploadManager());

    render(<AppShell me={me("agent")} onSignOut={vi.fn()} />);

    expect(screen.getByRole("button", { name: "Daily" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Private" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Open space library" })).not.toBeInTheDocument();
  });

  it("leaves horizontal shell boundaries to the title and status bars", () => {
    mocks.useWorkbenchController.mockReturnValue(workbench());
    mocks.useUploadManager.mockReturnValue(uploadManager());

    const view = render(<AppShell me={me("user")} onSignOut={vi.fn()} />);

    expect(view.container.querySelector("main")).not.toHaveClass("border-y", "border-seam");
  });

  it("keeps global shell regions mounted while changing surfaces", async () => {
    const user = userEvent.setup();
    mocks.useWorkbenchController.mockReturnValue(workbench());
    mocks.useUploadManager.mockReturnValue(uploadManager());

    const view = render(<AppShell me={me("user")} onSignOut={vi.fn()} />);
    const titleBar = screen.getByText("NoteGate").closest("header");
    const activityRail = screen.getByRole("complementary", { name: "Space navigation" });
    const statusBar = screen.getByText("ready").closest("footer");
    const surface = view.container.querySelector("main");

    expect(surface).not.toContainElement(activityRail);
    expect(surface?.parentElement).toContainElement(activityRail);

    await user.click(screen.getByRole("button", { name: "Open space library" }));

    expect(screen.getByText("NoteGate").closest("header")).toBe(titleBar);
    expect(screen.getByRole("complementary", { name: "Space navigation" })).toBe(activityRail);
    expect(screen.getByText("ready").closest("footer")).toBe(statusBar);
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

function uploadManager(overrides: Record<string, unknown> = {}) {
  return {
    tasks: [],
    activeCount: 0,
    queuedCount: 0,
    failedCount: 0,
    startUpload: vi.fn(),
    cancelUpload: vi.fn(),
    retryUpload: vi.fn(),
    dismissUpload: vi.fn(),
    ...overrides
  };
}
