import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { Me, Space } from "../api/types";
import { useUiStore } from "../stores/uiStore";
import { makeRestNode, makeSpace } from "../test/fixtures";
import { AppShell } from "./AppShell";

const mocks = vi.hoisted(() => ({
  useWorkbenchController: vi.fn(),
  recordingState: { status: "idle" },
  useUploadManager: vi.fn(),
  useUsageQuery: vi.fn(() => ({ data: undefined }))
}));

vi.mock("../features/workbench/useWorkbenchController", () => ({
  useWorkbenchController: mocks.useWorkbenchController
}));

vi.mock("../features/uploads/UploadProvider", () => ({
  useUploadManager: mocks.useUploadManager
}));

vi.mock("../features/recording/RecordingDock", () => ({
  RecordingDock: () => <div data-testid="recording-dock" />
}));

vi.mock("../features/recording/AudioRecordingContext", () => ({
  useAudioRecordingState: () => mocks.recordingState
}));

vi.mock("../features/spaces/useUsageQueries", () => ({
  useUsageQuery: mocks.useUsageQuery
}));

vi.mock("../features/editor/EditorArea", () => ({
  EditorArea: ({ canWriteActiveSpace, workspaceActive }: { canWriteActiveSpace: boolean; workspaceActive: boolean }) => (
    <div data-testid="editor-area" data-can-write={String(canWriteActiveSpace)} data-workspace-active={String(workspaceActive)} />
  )
}));
vi.mock("../features/nodes/PrimarySidebar", () => ({
  PrimarySidebar: ({ canWriteActiveSpace, onOpenNode, onOpenRecentNode, onOpenNodeInNewGroup }: {
    canWriteActiveSpace: boolean;
    onOpenNode: (node: typeof activeNode) => unknown;
    onOpenRecentNode: (node: typeof activeNode) => unknown;
    onOpenNodeInNewGroup: (node: typeof activeNode) => unknown;
  }) => (
    <div data-testid="primary-sidebar" data-can-write={String(canWriteActiveSpace)}>
      <button onClick={() => onOpenNode(activeNode)}>Open test node</button>
      <button onClick={() => onOpenRecentNode(activeNode)}>Open test recent</button>
      <button onClick={() => onOpenNodeInNewGroup(activeNode)}>Open test new group</button>
    </div>
  )
}));
vi.mock("../features/spaces/MobileSpaceBar", () => ({ MobileSpaceBar: () => null }));
vi.mock("../features/spaces/SpaceLibrary", () => ({
  SpaceLibrary: ({ spaces, usagePollingEnabled }: { spaces: Space[]; usagePollingEnabled: boolean }) => (
    <div data-testid="space-library" data-usage-polling-enabled={String(usagePollingEnabled)}>
      {spaces.map((item) => item.name).join(",")}
    </div>
  )
}));
vi.mock("./AuxiliarySidebar", () => ({ AuxiliarySidebar: () => null }));
vi.mock("../features/settings/SettingsView", () => ({
  SettingsView: ({ spaces, initialSpaceId, me }: { spaces: Space[]; initialSpaceId: string | null; me: Me }) => <div data-testid="settings-view" data-space-id={initialSpaceId ?? undefined} data-space-count={spaces.length} data-account-kind={me.account.kind} />
}));
vi.mock("../features/workbench/dialogs/DialogHost", () => ({
  DialogHost: () => <div data-testid="dialog-host" />
}));

const space = makeSpace({
  updated_at: "2026-07-10T00:00:00Z"
});

const activeNode = makeRestNode({
  space_id: space.id,
  parent_id: space.root_node_id,
  created_at: "2026-07-10T02:00:00Z",
  updated_at: "2026-07-10T02:12:00Z"
});

const privateSpace = makeSpace({
  ...space,
  id: "space-2",
  name: "Private",
  navigation_pinned: false,
  user_mcp_enabled: false,
  root_node_id: "root-2"
});

describe("AppShell", () => {
  beforeEach(() => {
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 1440, writable: true });
    useUiStore.setState(useUiStore.getInitialState(), true);
    mocks.recordingState = { status: "idle" };
  });

  it.each(["user", "agent"] as const)("supplies the current scope to settings for a %s account", async (kind) => {
    mocks.useWorkbenchController.mockReturnValue({ ...workbench(), settingsOpen: true });
    mocks.useUploadManager.mockReturnValue(uploadManager());
    render(<AppShell me={me(kind)} onSignOut={vi.fn()} />);
    const view = await screen.findByTestId("settings-view");
    expect(view).toHaveAttribute("data-space-id", space.id);
    expect(view).toHaveAttribute("data-space-count", "1");
    expect(view).toHaveAttribute("data-account-kind", kind);
    expect(screen.getByTestId("editor-area")).not.toBeVisible();
  });

  it.each(["recording", "paused"] as const)(
    "keeps reading surfaces available but disables changes while %s",
    async (status) => {
      const user = userEvent.setup();
      mocks.recordingState = { status };
      mocks.useWorkbenchController.mockReturnValue(workbench());
      mocks.useUploadManager.mockReturnValue(uploadManager());

      render(<AppShell me={me("user")} onSignOut={vi.fn()} />);

      expect(screen.getByTestId("primary-sidebar")).toHaveAttribute("data-can-write", "false");
      expect(screen.getByTestId("editor-area")).toHaveAttribute("data-can-write", "false");
      expect(await screen.findByTestId("recording-dock")).toBeInTheDocument();
      expect(screen.getByTestId("transfer-dock-stack")).toHaveClass(
        "md:fixed",
        "md:bottom-10",
        "md:right-3",
        "md:w-96",
        "md:flex-col"
      );
      await user.click(screen.getByRole("button", { name: "Settings" }));
      expect(screen.queryByTestId("settings-view")).not.toBeInTheDocument();
    }
  );

  it("loads the full settings surface when opened", async () => {
    mocks.useWorkbenchController.mockReturnValue({ ...workbench(), settingsOpen: true });
    mocks.useUploadManager.mockReturnValue(uploadManager());

    render(<AppShell me={me("user")} onSignOut={vi.fn()} />);

    expect(await screen.findByTestId("settings-view")).toBeInTheDocument();
  });

  it("loads the deferred dialog host for an active workbench dialog", async () => {
    mocks.useWorkbenchController.mockReturnValue({
      ...workbench(),
      dialog: {
        kind: "prompt",
        title: "New text",
        label: "Name",
        initial: "",
        onSubmit: vi.fn()
      }
    });
    mocks.useUploadManager.mockReturnValue(uploadManager());

    render(<AppShell me={me("user")} onSignOut={vi.fn()} />);

    expect(await screen.findByTestId("dialog-host")).toBeInTheDocument();
  });

  it("keeps inactive navigation-unpinned spaces out of the user rail while showing all spaces in the library", async () => {
    const user = userEvent.setup();
    mocks.useWorkbenchController.mockReturnValue({ ...workbench(), spaces: [space, privateSpace] });
    mocks.useUploadManager.mockReturnValue(uploadManager());

    render(<AppShell me={me("user")} onSignOut={vi.fn()} />);

    expect(screen.getByRole("button", { name: "Daily" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Private" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Open space library" }));

    const library = await screen.findByTestId("space-library");
    expect(library).toHaveTextContent("Daily,Private");
    expect(library).toHaveAttribute("data-usage-polling-enabled", "false");
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

    expect(mocks.useUsageQuery).toHaveBeenLastCalledWith(false);
    expect(screen.getByRole("button", { name: "Daily" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Private" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Open space library" })).not.toBeInTheDocument();
  });

  it("does not load status usage when the mobile status bar is hidden", () => {
    mocks.useWorkbenchController.mockReturnValue({ ...workbench(), isMobile: true });
    mocks.useUploadManager.mockReturnValue(uploadManager());

    render(<AppShell me={me("user")} onSignOut={vi.fn()} />);

    expect(mocks.useUsageQuery).toHaveBeenLastCalledWith(false);
  });

  it("lets the mobile library own usage polling while the status bar stays disabled", async () => {
    const user = userEvent.setup();
    mocks.useWorkbenchController.mockReturnValue({
      ...workbench(),
      isMobile: true,
      actions: { setSettingsOpen: vi.fn(), closeMobile: vi.fn() }
    });
    mocks.useUploadManager.mockReturnValue(uploadManager());

    render(<AppShell me={me("user")} onSignOut={vi.fn()} />);
    await user.click(screen.getByRole("button", { name: "Open space library" }));

    expect(screen.getByTestId("space-library")).toHaveAttribute("data-usage-polling-enabled", "true");
    expect(mocks.useUsageQuery).toHaveBeenLastCalledWith(false);
  });

  it("leaves horizontal shell boundaries to the title and status bars", () => {
    mocks.useWorkbenchController.mockReturnValue(workbench());
    mocks.useUploadManager.mockReturnValue(uploadManager());

    const view = render(<AppShell me={me("user")} onSignOut={vi.fn()} />);

    expect(view.container.firstElementChild).toHaveClass("font-ui");
    expect(view.container.querySelector("main")).not.toHaveClass("border-y", "border-seam");
  });

  it.each(["node", "recent", "new group"])("does not close newly opened mobile panels when an earlier %s open finishes", async (target) => {
    const user = userEvent.setup();
    let finishOpen!: (value: null) => void;
    const pendingOpen = new Promise<null>((resolve) => { finishOpen = resolve; });
    const closeMobile = vi.fn();
    const actions = {
      closeMobile,
      openNode: vi.fn(() => pendingOpen),
      openNodeInNewGroup: vi.fn(() => pendingOpen)
    };
    mocks.useWorkbenchController.mockReturnValue({
      ...workbench(), isMobile: true, mobileTreeOpen: true, actions
    });
    mocks.useUploadManager.mockReturnValue(uploadManager());
    const view = render(<AppShell me={me("user")} onSignOut={vi.fn()} />);

    await user.click(screen.getByRole("button", { name: `Open test ${target}` }));
    expect(closeMobile).toHaveBeenCalledOnce();

    mocks.useWorkbenchController.mockReturnValue({
      ...workbench(), activeSpace: privateSpace, isMobile: true, mobileAuxOpen: true, actions
    });
    view.rerender(<AppShell me={me("user")} onSignOut={vi.fn()} />);
    await act(async () => {
      finishOpen(null);
      await pendingOpen;
    });

    expect(closeMobile).toHaveBeenCalledOnce();
  });

  it("reads the docked Inspector width at the local frame boundary", () => {
    useUiStore.setState({ auxiliaryWidth: 380 });
    mocks.useWorkbenchController.mockReturnValue({
      ...workbench(),
      auxiliaryOpen: true,
      showAuxiliary: true
    });
    mocks.useUploadManager.mockReturnValue(uploadManager());

    render(<AppShell me={me("user")} onSignOut={vi.fn()} />);

    expect(document.getElementById("auxiliary-sidebar-panel")).toHaveStyle({ width: "380px" });
    expect(screen.getByRole("separator", { name: "Resize Inspector" })).toHaveAttribute(
      "aria-valuenow",
      "380"
    );
  });

  it("uses a dismissible right overlay when a non-mobile viewport cannot fit both panels", async () => {
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 800, writable: true });
    const toggleAuxiliary = vi.fn();
    mocks.useWorkbenchController.mockReturnValue({
      ...workbench(),
      auxiliaryOpen: true,
      showAuxiliary: true,
      actions: { setSettingsOpen: vi.fn(), toggleAuxiliary }
    });
    mocks.useUploadManager.mockReturnValue(uploadManager());
    render(<AppShell me={me("user")} onSignOut={vi.fn()} />);

    expect(document.getElementById("auxiliary-sidebar-panel")).toHaveClass("fixed", "right-0");
    expect(screen.queryByRole("separator", { name: "Resize Inspector" })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Close panel" }));
    expect(toggleAuxiliary).toHaveBeenCalledOnce();
  });

  it("fits docked panels on viewport resize and restores their preferred widths", () => {
    useUiStore.setState({ primaryWidth: 520, auxiliaryWidth: 520 });
    mocks.useWorkbenchController.mockReturnValue({ ...workbench(), auxiliaryOpen: true, showAuxiliary: true });
    mocks.useUploadManager.mockReturnValue(uploadManager());
    render(<AppShell me={me("user")} onSignOut={vi.fn()} />);

    act(() => {
      window.innerWidth = 900;
      window.dispatchEvent(new Event("resize"));
    });
    expect(document.getElementById("primary-sidebar-panel")).toHaveStyle({ width: "256px" });
    expect(document.getElementById("auxiliary-sidebar-panel")).toHaveStyle({ width: "280px" });
    expect(useUiStore.getState().primaryWidth).toBe(520);
    expect(useUiStore.getState().auxiliaryWidth).toBe(520);

    act(() => {
      window.innerWidth = 1440;
      window.dispatchEvent(new Event("resize"));
    });
    expect(document.getElementById("primary-sidebar-panel")).toHaveStyle({ width: "520px" });
    expect(document.getElementById("auxiliary-sidebar-panel")).toHaveStyle({ width: "520px" });
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

  it.each([false, true])("keeps the workspace mounted and inert during a Library visit (mobile: %s)", async (isMobile) => {
    const user = userEvent.setup();
    const actions = { setSettingsOpen: vi.fn(), selectSpace: vi.fn(), closeMobile: vi.fn() };
    mocks.useWorkbenchController.mockReturnValue({ ...workbench(), isMobile, actions });
    mocks.useUploadManager.mockReturnValue(uploadManager());
    const view = render(<AppShell me={me("user")} onSignOut={vi.fn()} />);
    const editor = screen.getByTestId("editor-area");
    expect(editor).toHaveAttribute("data-workspace-active", "true");

    await user.click(screen.getByRole("button", { name: "Open space library" }));

    expect(editor).toBeInTheDocument();
    expect(editor).not.toBeVisible();
    expect(editor).toHaveAttribute("data-workspace-active", "false");
    expect(editor.closest("[hidden]")).toHaveAttribute("inert");
    expect(editor.closest("[hidden]")).toHaveAttribute("aria-hidden", "true");

    await user.click(screen.getByRole("button", { name: /^Daily$/ }));

    expect(actions.selectSpace).toHaveBeenCalledWith(space);
    expect(screen.getByTestId("editor-area")).toBe(editor);
    expect(editor).toBeVisible();
    expect(editor).toHaveAttribute("data-workspace-active", "true");

    mocks.useWorkbenchController.mockReturnValue({ ...workbench(), isMobile, settingsOpen: true, actions });
    view.rerender(<AppShell me={me("user")} onSignOut={vi.fn()} />);
    expect(editor).toBeInTheDocument();
    expect(editor).not.toBeVisible();
    expect(editor).toHaveAttribute("data-workspace-active", "false");
    expect(editor.closest("[hidden]")).toHaveAttribute("inert");
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
    mobileTreeOpen: false,
    mobileAuxOpen: false,
    showAuxiliary: false,
    isMobile: false,
    settingsOpen: false,
    dialog: null,
    actions: { setSettingsOpen: vi.fn() }
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
