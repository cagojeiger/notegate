import { lazy, Suspense, useRef, useState } from "react";

import type { Me, Space } from "../api/types";
import { EditorArea } from "../features/editor/EditorArea";
import { MarkdownOutlineProvider } from "../features/editor/MarkdownOutlineContext";
import { useUsageQuery } from "../features/spaces/useUsageQueries";
import { MAX_EDITOR_GROUPS } from "../shared/model/workbenchLayout";
import { PrimarySidebar } from "../features/nodes/PrimarySidebar";
import type { TreeRevealRequest } from "../features/nodes/types";
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

type AppSurface = "workbench" | "library";

const SpaceLibrary = lazy(() => import("../features/spaces/SpaceLibrary").then((module) => ({ default: module.SpaceLibrary })));
const SettingsView = lazy(() => import("../features/settings/SettingsView").then((module) => ({ default: module.SettingsView })));
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
  const [surface, setSurface] = useState<AppSurface>("workbench");
  const [treeRevealRequest, setTreeRevealRequest] = useState<TreeRevealRequest | null>(null);
  const [settingsInspectorAvailable, setSettingsInspectorAvailable] = useState(false);
  const settingsOpener = useRef<HTMLElement | null>(null);
  const recentRequestId = useRef(0);
  const activeSpaceId = useRef(workbench.activeSpace?.id ?? null);
  activeSpaceId.current = workbench.activeSpace?.id ?? null;
  const { actions } = workbench;
  const recordingActive = recording.status !== "idle";
  const canWriteWorkbench = workbench.canWriteActiveSpace && !recordingActive;
  const canManageWorkbench = workbench.canManageActiveSpace && !recordingActive;
  const libraryAvailable = me.account.kind === "user";
  const librarySelected = libraryAvailable && surface === "library";
  const settingsOpen = workbench.settingsOpen;
  const libraryOpen = librarySelected && !settingsOpen;
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
  const overlayVisible = layout.primaryMode === "overlay" || layout.auxiliaryMode === "overlay";
  const closeMobilePanels = () => {
    if (workbench.isMobile) actions.closeMobile();
  };
  const closeOverlayPanels = () => {
    if (workbench.isMobile) actions.closeMobile();
    else if (layout.auxiliaryMode === "overlay") actions.toggleAuxiliary();
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
    settingsOpener.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    actions.setSettingsOpen(true);
  };
  const openLibrary = () => {
    if (recordingActive) return;
    closeMobilePanels();
    actions.setSettingsOpen(false);
    setSurface("library");
  };
  const selectWorkbenchSpace = (space: Parameters<typeof actions.selectSpace>[0]) => {
    if (recordingActive) return;
    actions.setSettingsOpen(false);
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
  const cancelTreeReveal = () => {
    recentRequestId.current += 1;
    setTreeRevealRequest(null);
  };
  const openNodeCore = (node: Parameters<typeof actions.openNode>[0]) => {
    closeMobilePanels();
    return actions.openNode(node);
  };
  const openNode = (node: Parameters<typeof actions.openNode>[0]) => {
    cancelTreeReveal();
    return openNodeCore(node);
  };
  const openRecentNode = (node: Parameters<typeof actions.openNode>[0]) => {
    cancelTreeReveal();
    const requestId = recentRequestId.current;
    void openNodeCore(node).then((reveal) => {
      if (requestId !== recentRequestId.current || activeSpaceId.current !== node.space_id || !reveal) return;
      setTreeRevealRequest({ spaceId: node.space_id, nodeId: reveal.target.id, path: reveal.target.path });
    });
  };
  const openNodeInNewGroup = (node: Parameters<typeof actions.openNodeInNewGroup>[0]) => {
    cancelTreeReveal();
    closeMobilePanels();
    return actions.openNodeInNewGroup(node);
  };
  const focusEditorGroup = (index: number) => {
    cancelTreeReveal();
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
        locationLabel={settingsOpen ? "Settings" : libraryOpen ? "Spaces" : undefined}
        showWorkbenchControls={!libraryOpen && !settingsOpen}
        theme={workbench.theme}
        primarySidebarOpen={workbench.isMobile ? workbench.mobileTreeOpen : workbench.primarySidebarOpen}
        auxiliaryOpen={(!settingsOpen || settingsInspectorAvailable) && (workbench.isMobile ? workbench.mobileAuxOpen : workbench.showAuxiliary)}
        auxiliaryDisabled={settingsOpen && !settingsInspectorAvailable}
        auxiliaryLabel={settingsOpen ? "Toggle settings inspector" : libraryOpen ? "Toggle space inspector" : undefined}
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
          settingsActive={settingsOpen}
          onOpenSettings={openSettings}
        />
        <main className="relative flex min-h-0 min-w-0 flex-1">
          {settingsOpen ? <Suspense fallback={<div className="grid flex-1 place-items-center text-sm text-muted" role="status">Preparing settings…</div>}>
            <SettingsView me={me} spaces={workbench.spaces} initialSpaceId={workbench.activeSpace?.id ?? null}
              onClose={() => { actions.setSettingsOpen(false); requestAnimationFrame(() => settingsOpener.current?.focus()); }} onSignOut={actions.handleSignOut}
              onResetSavedWorkspace={actions.confirmResetSavedWorkspace} isMobile={workbench.isMobile}
              inspectorOpen={workbench.isMobile ? workbench.mobileAuxOpen : workbench.showAuxiliary}
              onOpenInspector={openInspector} onCloseInspector={closeInspector} onInspectorAvailabilityChange={setSettingsInspectorAvailable} />
          </Suspense> : null}
          <div hidden={settingsOpen} className={settingsOpen ? "hidden" : "contents"} inert={settingsOpen} aria-hidden={settingsOpen || undefined}>
          {librarySelected ? (
            <Suspense fallback={<div className="grid min-h-0 flex-1 place-items-center text-sm text-muted" role="status">Preparing space library…</div>}>
              <SpaceLibrary
                spaces={workbench.spaces}
                activeSpace={workbench.activeSpace}
                canCreateSpace={workbench.canCreateSpace && !recordingActive}
                isMobile={workbench.isMobile}
                usagePollingEnabled={!statusUsageEnabled}
                inspectorOpen={!settingsOpen && (workbench.isMobile ? workbench.mobileAuxOpen : workbench.showAuxiliary)}
                onOpenInspector={openInspector}
                onCloseInspector={closeInspector}
                onOpenSpace={selectWorkbenchSpace}
                onCreateSpace={actions.promptCreateSpace}
              />
            </Suspense>
          ) : null}
            <div hidden={librarySelected} className={librarySelected ? "hidden" : "contents"} inert={librarySelected} aria-hidden={librarySelected || undefined}>
              <PrimarySidebarFrame id="primary-sidebar-panel" mode={layout.primaryMode} sidebarLayout={layout.sidebars}>
                <PrimarySidebar
                  activeSpace={workbench.activeSpace}
                  openedNodeId={workbench.activeNode?.id ?? null}
                  inspectedNodeId={workbench.inspectedNodeId}
                  expandedFolderIds={workbench.expandedFolderIds}
                  revealRequest={treeRevealRequest}
                  onRevealRequestHandled={(request) => setTreeRevealRequest((current) => current === request ? null : current)}
                  onCancelTreeReveal={cancelTreeReveal}
                  onOpenRecentNode={openRecentNode}
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
              <PrimarySidebarResizeHandle visible={layout.primaryMode === "docked"} sidebarLayout={layout.sidebars} />
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
              <AuxiliarySidebarResizeHandle visible={layout.auxiliaryMode === "docked"} sidebarLayout={layout.sidebars} />
              <AuxiliarySidebarFrame id="auxiliary-sidebar-panel" mode={layout.auxiliaryMode} sidebarLayout={layout.sidebars} isMobile={workbench.isMobile}>
                <AuxiliarySidebar
                  activeNode={workbench.inspectedNode}
                  activeGroupId={workbench.editorGroups[workbench.activeGroupIndex]?.id ?? null}
                  loadingNode={workbench.inspectorNodeLoading}
                  canManageActiveSpace={canManageWorkbench}
                  canSyncLinks={me.account.kind === "user" && canWriteWorkbench}
                  textEncryptionAvailable={workbench.activeSpace?.features.text_encryption ?? false}
                  writeLockAvailable={workbench.activeSpace?.features.write_lock ?? false}
                  externalAccessPolicyPending={actions.nodeExternalAccessPolicyPending}
                  writeLockPending={actions.nodeWriteLockPending}
                  textEncryptionPending={actions.textEncryptionPending}
                  onExternalAccessEnabledChange={actions.setNodeExternalAccessEnabled}
                  onWriteLockedChange={actions.setNodeWriteLocked}
                  onTextEncryptionEnabledChange={actions.setTextEncryptionEnabled}
                  onOpenLinkedNode={(nodeId, sourceNodeId) => {
                    void actions.openNodeById(nodeId, sourceNodeId);
                  }}
                  onOutlineNavigate={closeMobilePanels}
                />
              </AuxiliarySidebarFrame>
              <PanelOverlay visible={overlayVisible} onClose={closeOverlayPanels} isMobile={workbench.isMobile} />
            </div>
          </div>
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
          settingsActive={settingsOpen}
          onOpenSettings={openSettings}
        />
        <WorkspaceStatusBar
          activeSpace={workbench.activeSpace}
          usageEnabled={statusUsageEnabled}
        />
      </div>
      <Toast />
      {workbench.dialog ? <Suspense fallback={null}><DialogHost dialog={workbench.dialog} onClose={() => actions.setDialog(null)} /></Suspense> : null}
    </div>
    </MarkdownOutlineProvider>
  );
}
