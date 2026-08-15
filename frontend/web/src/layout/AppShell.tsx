import { lazy, Suspense, useState } from "react";

import type { Me, Space } from "../api/types";
import { canViewAuditEvents } from "../auth/permissions";
import { EditorArea } from "../features/editor/EditorArea";
import { MarkdownOutlineProvider } from "../features/editor/MarkdownOutlineContext";
import { useUsageQuery } from "../features/spaces/useUsageQueries";
import { MAX_EDITOR_GROUPS } from "../shared/model/workbenchLayout";
import { PrimarySidebar } from "../features/nodes/PrimarySidebar";
import { useAudioRecordingState } from "../features/recording/AudioRecordingContext";
import { ActivityRail } from "../features/spaces/ActivityRail";
import { MobileSpaceBar } from "../features/spaces/MobileSpaceBar";
import { mergeVisibleSpaceOrder } from "../features/spaces/spaceReorder";
import { UploadProgressDock } from "../features/uploads/UploadProgressDock";
import { useWorkbenchController } from "../features/workbench/useWorkbenchController";
import { AuxiliarySidebar } from "./AuxiliarySidebar";
import { FullScreenStatus } from "./FullScreenStatus";
import { StatusBar } from "./StatusBar";
import { TitleBar } from "./TitleBar";
import { Toast } from "./Toast";
import { AuxiliarySidebarFrame, AuxiliarySidebarResizeHandle, PanelOverlay, PrimarySidebarFrame, PrimarySidebarResizeHandle } from "./WorkbenchFrames";
import { useWorkbenchLayout } from "./workbenchLayout";

type AppShellProps = {
  me: Me;
  onSignOut: () => void;
};

type HistoryScope = {
  initialSpaceId: string | null;
};

type AppSurface = "workbench" | "library";

const SpaceLibrary = lazy(() => import("../features/spaces/SpaceLibrary").then((module) => ({ default: module.SpaceLibrary })));
const EventHistoryModal = lazy(() => import("../features/events/EventHistoryModal").then((module) => ({ default: module.EventHistoryModal })));
const SettingsModal = lazy(() => import("../features/settings/SettingsModal").then((module) => ({ default: module.SettingsModal })));
const DialogHost = lazy(() => import("../features/workbench/dialogs/DialogHost").then((module) => ({ default: module.DialogHost })));
const RecordingDock = lazy(() => import("../features/recording/RecordingDock").then((module) => ({ default: module.RecordingDock })));

function WorkspaceStatusBar({
  activeSpace,
  usageEnabled
}: {
  activeSpace: Space | null;
  usageEnabled: boolean;
}) {
  const usageQuery = useUsageQuery(usageEnabled);
  const usage = activeSpace
    ? usageQuery.data?.spaces.find((candidate) => candidate.id === activeSpace.id)
    : undefined;

  return <StatusBar activeSpace={activeSpace} usage={usage} />;
}

export function AppShell({ me, onSignOut }: AppShellProps) {
  const workbench = useWorkbenchController({ me, onSignOut });
  const recording = useAudioRecordingState();
  const [historyScope, setHistoryScope] = useState<HistoryScope | null>(null);
  const [surface, setSurface] = useState<AppSurface>("workbench");
  const { actions } = workbench;
  const recordingActive = recording.status !== "idle";
  const canWriteWorkbench = workbench.canWriteActiveSpace && !recordingActive;
  const canManageWorkbench = workbench.canManageActiveSpace && !recordingActive;
  const libraryAvailable = me.account.kind === "user";
  const libraryOpen = libraryAvailable && surface === "library";
  const statusUsageEnabled = me.account.kind === "user" && !workbench.isMobile;
  const railSpaces = me.account.kind === "user"
    ? workbench.spaces.filter((space) => space.navigation_pinned)
    : workbench.spaces;
  const layout = useWorkbenchLayout({
    isMobile: workbench.isMobile,
    primaryOpen: workbench.isMobile ? workbench.mobileTreeOpen : workbench.primarySidebarOpen,
    auxiliaryOpen: workbench.isMobile ? workbench.mobileAuxOpen : workbench.showAuxiliary,
    editorGroupCount: workbench.editorGroups.length
  });
  const mobileOverlayVisible = workbench.isMobile && (layout.primaryMode === "overlay" || layout.auxiliaryMode === "overlay");
  const closeMobilePanels = () => {
    if (workbench.isMobile) actions.closeMobile();
  };
  const openInspector = () => {
    if (workbench.isMobile) {
      if (!workbench.mobileAuxOpen) actions.toggleMobileAux();
    } else if (!workbench.showAuxiliary) {
      actions.toggleAuxiliary();
    }
  };
  const closeInspector = () => {
    if (workbench.isMobile) {
      if (workbench.mobileAuxOpen) actions.toggleMobileAux();
    } else if (workbench.showAuxiliary) {
      actions.toggleAuxiliary();
    }
  };
  const openSettings = () => {
    if (recordingActive) return;
    closeMobilePanels();
    actions.setSettingsOpen(true);
  };
  const openLibrary = () => {
    if (recordingActive) return;
    closeMobilePanels();
    setSurface("library");
  };
  const selectWorkbenchSpace = (space: Parameters<typeof actions.selectSpace>[0]) => {
    if (recordingActive) return;
    actions.selectSpace(space);
    setSurface("workbench");
  };
  const reorderRailSpaces = (orderedSpaces: Parameters<typeof actions.reorderSpaces>[0]) => {
    if (recordingActive) return;
    actions.reorderSpaces(
      me.account.kind === "user"
        ? mergeVisibleSpaceOrder(workbench.spaces, orderedSpaces)
        : orderedSpaces
    );
  };
  const openHistory = () => {
    if (recordingActive) return;
    closeMobilePanels();
    setHistoryScope({ initialSpaceId: workbench.activeSpace?.id ?? null });
  };
  const openNode = async (node: Parameters<typeof actions.openNode>[0]) => {
    try {
      await actions.openNode(node);
    } finally {
      closeMobilePanels();
    }
  };
  const openNodeInNewGroup = async (node: Parameters<typeof actions.openNodeInNewGroup>[0]) => {
    try {
      await actions.openNodeInNewGroup(node);
    } finally {
      closeMobilePanels();
    }
  };
  const focusEditorGroup = (index: number) => {
    actions.focusGroup(index);
    workbench.inspectNode(workbench.editorGroups[index]?.node ?? null);
  };

  if (workbench.loading) return <FullScreenStatus label="Loading spaces" />;
  if (workbench.error) return <FullScreenStatus label="Could not load spaces" detail={workbench.error} />;

  return (
    <MarkdownOutlineProvider>
    <div className="flex h-full flex-col overflow-hidden bg-bg font-ui text-text">
      <TitleBar
        activeSpace={workbench.activeSpace}
        locationLabel={libraryOpen ? "Spaces" : undefined}
        showWorkbenchControls={!libraryOpen}
        theme={workbench.theme}
        primarySidebarOpen={workbench.isMobile ? workbench.mobileTreeOpen : workbench.primarySidebarOpen}
        auxiliaryOpen={workbench.isMobile ? workbench.mobileAuxOpen : workbench.showAuxiliary}
        auxiliaryLabel={libraryOpen ? "Toggle space inspector" : undefined}
        editorGroupCount={workbench.editorGroups.length}
        onAddGroup={actions.addGroup}
        onToggleTheme={actions.toggleTheme}
        onTogglePrimarySidebar={workbench.isMobile ? actions.toggleMobileTree : actions.togglePrimarySidebar}
        onToggleAuxiliary={workbench.isMobile ? actions.toggleMobileAux : actions.toggleAuxiliary}
      />
      <div className="relative flex min-h-0 min-w-0 flex-1">
        <ActivityRail
          spaces={railSpaces}
          activeSpace={workbench.activeSpace}
          canCreateSpace={workbench.canCreateSpace && !recordingActive}
          canManageSpaces={workbench.canCreateSpace && !recordingActive}
          navigationLocked={recordingActive}
          onSelectSpace={selectWorkbenchSpace}
          onReorderSpaces={reorderRailSpaces}
          onCreateSpace={recordingActive ? () => undefined : actions.promptCreateSpace}
          onRenameSpace={actions.promptRenameSpace}
          onDeleteSpace={actions.confirmDeleteSpace}
          onOpenLibrary={libraryAvailable ? openLibrary : undefined}
          libraryActive={libraryOpen}
          onOpenHistory={openHistory}
          onOpenSettings={openSettings}
        />
        <main className="relative flex min-h-0 min-w-0 flex-1">
          {libraryOpen ? (
            <Suspense fallback={<div className="grid min-h-0 flex-1 place-items-center text-sm text-muted" role="status">Preparing space library…</div>}>
              <SpaceLibrary
                spaces={workbench.spaces}
                activeSpace={workbench.activeSpace}
                isMobile={workbench.isMobile}
                usagePollingEnabled={!statusUsageEnabled}
                inspectorOpen={workbench.isMobile ? workbench.mobileAuxOpen : workbench.showAuxiliary}
                onOpenInspector={openInspector}
                onCloseInspector={closeInspector}
                onOpenSpace={selectWorkbenchSpace}
                onCreateSpace={actions.promptCreateSpace}
              />
            </Suspense>
          ) : (
            <>
              <PrimarySidebarFrame id="primary-sidebar-panel" mode={layout.primaryMode}>
                <PrimarySidebar
                  activeSpace={workbench.activeSpace}
                  openedNodeId={workbench.activeNode?.id ?? null}
                  inspectedNodeId={workbench.inspectedNodeId}
                  expandedFolderIds={workbench.expandedFolderIds}
                  onToggleFolder={actions.toggleFolder}
                  onInspectNode={workbench.inspectNode}
                  onOpenNode={(node) => { void openNode(node); }}
                  onOpenNodeInNewGroup={(node) => { void openNodeInNewGroup(node); }}
                  onCreateFolder={() => actions.promptCreateNode("folder")}
                  onCreateText={() => actions.promptCreateNode("text")}
                  onRecordAudio={actions.recordAudio}
                  onFileSelected={actions.handleFileSelected}
                  onRenameSpace={actions.promptRenameSpace}
                  onDeleteSpace={actions.confirmDeleteSpace}
                  onRenameNode={actions.promptRenameNode}
                  onMoveNode={actions.promptMoveNode}
                  onMoveNodeToFolder={actions.moveNodeToFolder}
                  onDeleteNode={actions.confirmDeleteNode}
                  onDownloadFile={actions.downloadFileNode}
                  onCollapseTree={actions.collapseTree}
                  onCreateInFolder={actions.promptCreateInFolder}
                  onUploadInFolder={actions.uploadInFolder}
                  canWriteActiveSpace={canWriteWorkbench}
                  canManageActiveSpace={canManageWorkbench}
                  canOpenInNewGroup={workbench.editorGroups.length < MAX_EDITOR_GROUPS}
                />
              </PrimarySidebarFrame>
              <PrimarySidebarResizeHandle visible={layout.primaryMode === "docked"} />
              <EditorArea
                groups={workbench.editorGroups}
                activeGroupIndex={workbench.activeGroupIndex}
                presentation={layout.editorPresentation}
                visibleGroupCount={layout.visibleEditorGroupCount}
                activeSpace={workbench.activeSpace}
                onFocusGroup={focusEditorGroup}
                onNavigateEditorGroup={(groupId, direction) => { void actions.navigateEditorGroup(groupId, direction); }}
                navigatingGroupIds={actions.navigatingGroupIds}
                onOpenNode={(node) => { void openNode(node); }}
                onOpenNodeInNewGroup={(node) => { void openNodeInNewGroup(node); }}
                onOpenMarkdownLink={(groupId, node, path) => { void actions.openMarkdownLink(groupId, node, path); }}
                onCloseGroup={actions.closeGroup}
                onSetGroupMode={actions.setGroupMode}
                onCreateFolder={() => actions.promptCreateNode("folder")}
                onCreateText={() => actions.promptCreateNode("text")}
                onRecordAudio={actions.recordAudio}
                onFileSelected={actions.handleFileSelected}
                onRenameNode={actions.promptRenameNode}
                onMoveNode={actions.promptMoveNode}
                onDeleteNode={actions.confirmDeleteNode}
                onDownloadFile={actions.downloadFileNode}
                canWriteActiveSpace={canWriteWorkbench}
              />
              <AuxiliarySidebarResizeHandle visible={layout.auxiliaryMode === "docked"} />
              <AuxiliarySidebarFrame id="auxiliary-sidebar-panel" mode={layout.auxiliaryMode}>
                <AuxiliarySidebar
                  activeNode={workbench.inspectedNode}
                  activeGroupId={workbench.editorGroups[workbench.activeGroupIndex]?.id ?? null}
                  loadingNode={workbench.inspectorNodeLoading}
                  canManageActiveSpace={canManageWorkbench}
                  textEncryptionAvailable={workbench.activeSpace?.features.text_encryption ?? false}
                  writeLockAvailable={workbench.activeSpace?.features.write_lock ?? false}
                  searchPolicyPending={actions.nodeSearchPolicyPending}
                  writeLockPending={actions.nodeWriteLockPending}
                  textEncryptionPending={actions.textEncryptionPending}
                  onSearchEnabledChange={actions.setNodeSearchEnabled}
                  onWriteLockedChange={actions.setNodeWriteLocked}
                  onTextEncryptionEnabledChange={actions.setTextEncryptionEnabled}
                  onOutlineNavigate={closeMobilePanels}
                />
              </AuxiliarySidebarFrame>
              <PanelOverlay visible={mobileOverlayVisible} onClose={closeMobilePanels} />
            </>
          )}
        </main>
      </div>
      <div className="shrink-0">
        <div
          data-testid="transfer-dock-stack"
          className="z-20 shrink-0 md:pointer-events-none md:fixed md:bottom-10 md:right-3 md:flex md:w-96 md:flex-col md:gap-2"
        >
          {recordingActive ? <Suspense fallback={null}><RecordingDock /></Suspense> : null}
          <UploadProgressDock />
        </div>
        <MobileSpaceBar
          spaces={railSpaces}
          activeSpace={workbench.activeSpace}
          canCreateSpace={workbench.canCreateSpace && !recordingActive}
          navigationLocked={recordingActive}
          onSelectSpace={selectWorkbenchSpace}
          onCreateSpace={recordingActive ? () => undefined : actions.promptCreateSpace}
          onOpenLibrary={libraryAvailable ? openLibrary : undefined}
          libraryActive={libraryOpen}
          onOpenHistory={openHistory}
          onOpenSettings={openSettings}
        />
        <WorkspaceStatusBar
          activeSpace={workbench.activeSpace}
          usageEnabled={statusUsageEnabled}
        />
      </div>
      <Toast />
      {historyScope ? <Suspense fallback={null}><EventHistoryModal spaces={workbench.spaces} initialSpaceId={historyScope.initialSpaceId} canViewAuditEvents={canViewAuditEvents(me)} onClose={() => setHistoryScope(null)} /></Suspense> : null}
      {workbench.settingsOpen ? <Suspense fallback={null}><SettingsModal me={me} onClose={() => actions.setSettingsOpen(false)} onSignOut={actions.handleSignOut} onResetSavedWorkspace={actions.confirmResetSavedWorkspace} /></Suspense> : null}
      {workbench.dialog ? <Suspense fallback={null}><DialogHost dialog={workbench.dialog} onClose={() => actions.setDialog(null)} /></Suspense> : null}
    </div>
    </MarkdownOutlineProvider>
  );
}
