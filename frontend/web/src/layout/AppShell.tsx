import { lazy, Suspense, useState } from "react";

import type { Me, Space } from "../api/types";
import { canViewAuditEvents } from "../auth/permissions";
import { EditorArea } from "../features/editor/EditorArea";
import { MarkdownOutlineProvider } from "../features/editor/MarkdownOutlineContext";
import { EventHistoryModal } from "../features/events/EventHistoryModal";
import { SettingsModal } from "../features/settings/SettingsModal";
import { useUsageQuery } from "../features/spaces/useUsageQueries";
import { MAX_EDITOR_GROUPS } from "../shared/model/workbenchLayout";
import { PrimarySidebar } from "../features/nodes/PrimarySidebar";
import { ActivityRail } from "../features/spaces/ActivityRail";
import { MobileSpaceBar } from "../features/spaces/MobileSpaceBar";
import { mergeVisibleSpaceOrder } from "../features/spaces/spaceReorder";
import { UploadProgressDock } from "../features/uploads/UploadProgressDock";
import { DialogHost } from "../features/workbench/dialogs/DialogHost";
import { useWorkbenchController } from "../features/workbench/useWorkbenchController";
import { AuxiliarySidebar } from "./AuxiliarySidebar";
import { FullScreenStatus } from "./FullScreenStatus";
import { StatusBar } from "./StatusBar";
import { TitleBar } from "./TitleBar";
import { Toast } from "./Toast";
import { AuxiliarySidebarFrame, PanelOverlay, PrimarySidebarFrame, PrimarySidebarResizeHandle } from "./WorkbenchFrames";
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
  const [historyScope, setHistoryScope] = useState<HistoryScope | null>(null);
  const [surface, setSurface] = useState<AppSurface>("workbench");
  const { actions } = workbench;
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
    closeMobilePanels();
    actions.setSettingsOpen(true);
  };
  const openLibrary = () => {
    closeMobilePanels();
    setSurface("library");
  };
  const selectWorkbenchSpace = (space: Parameters<typeof actions.selectSpace>[0]) => {
    actions.selectSpace(space);
    setSurface("workbench");
  };
  const reorderRailSpaces = (orderedSpaces: Parameters<typeof actions.reorderSpaces>[0]) => {
    actions.reorderSpaces(
      me.account.kind === "user"
        ? mergeVisibleSpaceOrder(workbench.spaces, orderedSpaces)
        : orderedSpaces
    );
  };
  const openHistory = () => {
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
    <div className="flex h-full flex-col overflow-hidden bg-bg text-text">
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
          canCreateSpace={workbench.canCreateSpace}
          canManageSpaces={workbench.canCreateSpace}
          onSelectSpace={selectWorkbenchSpace}
          onReorderSpaces={reorderRailSpaces}
          onCreateSpace={actions.promptCreateSpace}
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
              <PrimarySidebarFrame id="primary-sidebar-panel" mode={layout.primaryMode} width={workbench.primaryWidth}>
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
                  canWriteActiveSpace={workbench.canWriteActiveSpace}
                  canManageActiveSpace={workbench.canManageActiveSpace}
                  canOpenInNewGroup={workbench.editorGroups.length < MAX_EDITOR_GROUPS}
                />
              </PrimarySidebarFrame>
              <PrimarySidebarResizeHandle
                visible={layout.primaryMode === "docked"}
                value={workbench.primaryWidth}
                onPointerDown={actions.startPrimaryResize}
                onValueChange={actions.setPrimaryWidth}
              />
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
                onFileSelected={actions.handleFileSelected}
                onRenameNode={actions.promptRenameNode}
                onMoveNode={actions.promptMoveNode}
                onDeleteNode={actions.confirmDeleteNode}
                onDownloadFile={actions.downloadFileNode}
                canWriteActiveSpace={workbench.canWriteActiveSpace}
              />
              <AuxiliarySidebarFrame mode={layout.auxiliaryMode}>
                <AuxiliarySidebar
                  activeNode={workbench.inspectedNode}
                  activeGroupId={workbench.editorGroups[workbench.activeGroupIndex]?.id ?? null}
                  loadingNode={workbench.inspectorNodeLoading}
                  canWriteActiveSpace={workbench.canWriteActiveSpace}
                  canManageActiveSpace={workbench.canManageActiveSpace}
                  textEncryptionAvailable={workbench.activeSpace?.features.text_encryption ?? false}
                  writeLockAvailable={workbench.activeSpace?.features.write_lock ?? false}
                  searchPolicyPending={actions.nodeSearchPolicyPending}
                  writeLockPending={actions.nodeWriteLockPending}
                  textEncryptionPending={actions.textEncryptionPending}
                  onReplaceMetadata={actions.promptReplaceMetadata}
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
        <UploadProgressDock />
        <MobileSpaceBar
          spaces={railSpaces}
          activeSpace={workbench.activeSpace}
          canCreateSpace={workbench.canCreateSpace}
          onSelectSpace={selectWorkbenchSpace}
          onCreateSpace={actions.promptCreateSpace}
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
      {historyScope ? <EventHistoryModal spaces={workbench.spaces} initialSpaceId={historyScope.initialSpaceId} canViewAuditEvents={canViewAuditEvents(me)} onClose={() => setHistoryScope(null)} /> : null}
      {workbench.settingsOpen ? <SettingsModal me={me} onClose={() => actions.setSettingsOpen(false)} onSignOut={actions.handleSignOut} onResetSavedWorkspace={actions.confirmResetSavedWorkspace} /> : null}
      <DialogHost dialog={workbench.dialog} onClose={() => actions.setDialog(null)} />
    </div>
    </MarkdownOutlineProvider>
  );
}
