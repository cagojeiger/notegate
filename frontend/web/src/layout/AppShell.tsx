import { useState } from "react";

import type { Me } from "../api/types";
import { canViewAuditEvents } from "../auth/permissions";
import { EditorArea } from "../features/editor/EditorArea";
import { EventHistoryModal } from "../features/events/EventHistoryModal";
import { SettingsModal } from "../features/settings/SettingsModal";
import { MAX_EDITOR_GROUPS } from "../stores/uiStore";
import { PrimarySidebar } from "../features/nodes/PrimarySidebar";
import { ActivityRail } from "../features/spaces/ActivityRail";
import { MobileSpaceBar } from "../features/spaces/MobileSpaceBar";
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

export function AppShell({ me, onSignOut }: AppShellProps) {
  const workbench = useWorkbenchController({ me, onSignOut });
  const [historyScope, setHistoryScope] = useState<HistoryScope | null>(null);
  const { actions } = workbench;
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
  const openSettings = () => {
    closeMobilePanels();
    actions.setSettingsOpen(true);
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

  if (workbench.loading) return <FullScreenStatus label="Loading spaces" />;
  if (workbench.error) return <FullScreenStatus label="Could not load spaces" detail={workbench.error} />;

  return (
    <div className="flex h-full flex-col overflow-hidden bg-bg text-text">
      <TitleBar
        activeSpace={workbench.activeSpace}
        theme={workbench.theme}
        primarySidebarOpen={workbench.isMobile ? workbench.mobileTreeOpen : workbench.primarySidebarOpen}
        auxiliaryOpen={workbench.isMobile ? workbench.mobileAuxOpen : workbench.showAuxiliary}
        editorGroupCount={workbench.editorGroups.length}
        onAddGroup={actions.addGroup}
        onToggleTheme={actions.toggleTheme}
        onTogglePrimarySidebar={workbench.isMobile ? actions.toggleMobileTree : actions.togglePrimarySidebar}
        onToggleAuxiliary={workbench.isMobile ? actions.toggleMobileAux : actions.toggleAuxiliary}
      />
      <main className="relative flex min-h-0 flex-1 border-y border-seam">
        <ActivityRail spaces={workbench.spaces} activeSpace={workbench.activeSpace} canCreateSpace={workbench.canCreateSpace} canManageSpaces={workbench.canCreateSpace} onSelectSpace={actions.selectSpace} onReorderSpaces={actions.reorderSpaces} onCreateSpace={actions.promptCreateSpace} onRenameSpace={actions.promptRenameSpace} onDeleteSpace={actions.confirmDeleteSpace} onOpenHistory={openHistory} onOpenSettings={openSettings} />
        <PrimarySidebarFrame mode={layout.primaryMode} width={workbench.primaryWidth}>
          <PrimarySidebar
            activeSpace={workbench.activeSpace}
            activeNodeId={workbench.activeNode?.id ?? null}
            expandedFolderIds={workbench.expandedFolderIds}
            onToggleFolder={actions.toggleFolder}
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
        <PrimarySidebarResizeHandle visible={layout.primaryMode === "docked"} onPointerDown={actions.startPrimaryResize} />
        <EditorArea
          groups={workbench.editorGroups}
          activeGroupIndex={workbench.activeGroupIndex}
          presentation={layout.editorPresentation}
          visibleGroupCount={layout.visibleEditorGroupCount}
          activeSpace={workbench.activeSpace}
          onFocusGroup={actions.focusGroup}
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
          <AuxiliarySidebar activeNode={workbench.activeNode} canWriteActiveSpace={workbench.canWriteActiveSpace} onReplaceMetadata={actions.promptReplaceMetadata} />
        </AuxiliarySidebarFrame>
        <PanelOverlay visible={mobileOverlayVisible} onClose={closeMobilePanels} />
      </main>
      <UploadProgressDock />
      <MobileSpaceBar spaces={workbench.spaces} activeSpace={workbench.activeSpace} canCreateSpace={workbench.canCreateSpace} onSelectSpace={actions.selectSpace} onCreateSpace={actions.promptCreateSpace} onOpenHistory={openHistory} onOpenSettings={openSettings} />
      <StatusBar activeSpace={workbench.activeSpace} />
      <Toast />
      {historyScope ? <EventHistoryModal spaces={workbench.spaces} initialSpaceId={historyScope.initialSpaceId} canViewAuditEvents={canViewAuditEvents(me)} onClose={() => setHistoryScope(null)} /> : null}
      {workbench.settingsOpen ? <SettingsModal me={me} onClose={() => actions.setSettingsOpen(false)} onSignOut={actions.handleSignOut} onResetSavedWorkspace={actions.confirmResetSavedWorkspace} /> : null}
      <DialogHost dialog={workbench.dialog} onClose={() => actions.setDialog(null)} />
    </div>
  );
}
