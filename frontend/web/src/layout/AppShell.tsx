import type { Me } from "../api/types";
import { EditorArea } from "../features/editor/EditorArea";
import { MAX_EDITOR_GROUPS } from "../stores/uiStore";
import { PrimarySidebar } from "../features/nodes/PrimarySidebar";
import { ActivityRail } from "../features/spaces/ActivityRail";
import { MobileSpaceBar } from "../features/spaces/MobileSpaceBar";
import { useWorkbenchController } from "../features/workbench/useWorkbenchController";
import { AuxiliarySidebar } from "./AuxiliarySidebar";
import { DialogHost } from "./dialogs/DialogHost";
import { FullScreenStatus } from "./FullScreenStatus";
import { SettingsModal } from "./SettingsModal";
import { StatusBar } from "./StatusBar";
import { TitleBar } from "./TitleBar";
import { Toast } from "./Toast";
import { AuxiliarySidebarFrame, MobilePanelOverlay, PrimarySidebarFrame, PrimarySidebarResizeHandle } from "./WorkbenchFrames";

type AppShellProps = {
  me: Me;
  onSignOut: () => void;
};

export function AppShell({ me, onSignOut }: AppShellProps) {
  const workbench = useWorkbenchController({ me, onSignOut });
  const { actions } = workbench;
  const openSettings = () => {
    actions.closeMobile();
    actions.setSettingsOpen(true);
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
        <ActivityRail spaces={workbench.spaces} activeSpace={workbench.activeSpace} canCreateSpace={workbench.canCreateSpace} canManageSpaces={workbench.canCreateSpace} onSelectSpace={actions.selectSpace} onReorderSpaces={actions.reorderSpaces} onCreateSpace={actions.promptCreateSpace} onRenameSpace={actions.promptRenameSpace} onDeleteSpace={actions.confirmDeleteSpace} onOpenSettings={openSettings} />
        <PrimarySidebarFrame isMobile={workbench.isMobile} open={workbench.primarySidebarOpen} mobileOpen={workbench.mobileTreeOpen} width={workbench.primaryWidth}>
          <PrimarySidebar
            activeSpace={workbench.activeSpace}
            activeNodeId={workbench.activeNode?.id ?? null}
            expandedFolderIds={workbench.expandedFolderIds}
            onToggleFolder={actions.toggleFolder}
            onOpenNode={actions.openNode}
            onOpenNodeInNewGroup={actions.openNodeInNewGroup}
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
        <PrimarySidebarResizeHandle visible={workbench.primarySidebarOpen} onPointerDown={actions.startPrimaryResize} />
        <EditorArea
          groups={workbench.editorGroups}
          activeGroupIndex={workbench.activeGroupIndex}
          activeSpace={workbench.activeSpace}
          onFocusGroup={actions.focusGroup}
          onOpenNode={actions.openNode}
          onOpenNodeInNewGroup={actions.openNodeInNewGroup}
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
        <AuxiliarySidebarFrame open={workbench.showAuxiliary} mobileOpen={workbench.mobileAuxOpen}>
          <AuxiliarySidebar activeNode={workbench.activeNode} canWriteActiveSpace={workbench.canWriteActiveSpace} onReplaceMetadata={actions.promptReplaceMetadata} />
        </AuxiliarySidebarFrame>
        <MobilePanelOverlay visible={workbench.mobileTreeOpen || workbench.mobileAuxOpen} onClose={actions.closeMobile} />
      </main>
      <MobileSpaceBar spaces={workbench.spaces} activeSpace={workbench.activeSpace} canCreateSpace={workbench.canCreateSpace} onSelectSpace={actions.selectSpace} onCreateSpace={actions.promptCreateSpace} onOpenSettings={openSettings} />
      <StatusBar activeSpace={workbench.activeSpace} />
      <Toast />
      {workbench.settingsOpen ? <SettingsModal me={me} onClose={() => actions.setSettingsOpen(false)} onSignOut={actions.handleSignOut} onResetSavedWorkspace={actions.confirmResetSavedWorkspace} /> : null}
      <DialogHost dialog={workbench.dialog} onClose={() => actions.setDialog(null)} />
    </div>
  );
}
