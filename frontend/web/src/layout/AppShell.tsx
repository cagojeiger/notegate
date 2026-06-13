import { EditorArea } from "../features/editor/EditorArea";
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
  onSignOut: () => void;
};

export function AppShell({ onSignOut }: AppShellProps) {
  const workbench = useWorkbenchController({ onSignOut });
  const { actions } = workbench;

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
        <ActivityRail spaces={workbench.spaces} activeSpace={workbench.activeSpace} onSelectSpace={actions.selectSpace} onCreateSpace={actions.promptCreateSpace} onOpenSettings={() => actions.setSettingsOpen(true)} />
        <PrimarySidebarFrame isMobile={workbench.isMobile} open={workbench.primarySidebarOpen} mobileOpen={workbench.mobileTreeOpen} width={workbench.primaryWidth}>
          <PrimarySidebar
            activeSpace={workbench.activeSpace}
            activeNodeId={workbench.activeNode?.id ?? null}
            expandedFolderIds={workbench.expandedFolderIds}
            onToggleFolder={actions.toggleFolder}
            onOpenNode={actions.openNode}
            onCreateFolder={() => actions.promptCreateNode("folder")}
            onCreateText={() => actions.promptCreateNode("text")}
            onFileSelected={actions.handleFileSelected}
            onRenameSpace={actions.promptRenameSpace}
            onDeleteSpace={actions.confirmDeleteSpace}
            onRenameNode={actions.promptRenameNode}
            onDeleteNode={actions.confirmDeleteNode}
            onCollapseTree={actions.collapseTree}
            onCreateInFolder={actions.promptCreateInFolder}
            onUploadInFolder={actions.uploadInFolder}
          />
        </PrimarySidebarFrame>
        <PrimarySidebarResizeHandle visible={workbench.primarySidebarOpen} onPointerDown={actions.startPrimaryResize} />
        <EditorArea
          groups={workbench.editorGroups}
          activeGroupIndex={workbench.activeGroupIndex}
          activeSpace={workbench.activeSpace}
          onFocusGroup={actions.focusGroup}
          onCloseGroup={actions.closeGroup}
          onSetGroupMode={actions.setGroupMode}
          onCreateFolder={() => actions.promptCreateNode("folder")}
          onCreateText={() => actions.promptCreateNode("text")}
          onFileSelected={actions.handleFileSelected}
          onRenameNode={actions.promptRenameNode}
          onMoveNode={actions.promptMoveNode}
          onDeleteNode={actions.confirmDeleteNode}
        />
        <AuxiliarySidebarFrame open={workbench.showAuxiliary} mobileOpen={workbench.mobileAuxOpen}>
          <AuxiliarySidebar activeNode={workbench.activeNode} onReplaceMetadata={actions.promptReplaceMetadata} />
        </AuxiliarySidebarFrame>
        <MobilePanelOverlay visible={workbench.mobileTreeOpen || workbench.mobileAuxOpen} onClose={actions.closeMobile} />
      </main>
      <MobileSpaceBar spaces={workbench.spaces} activeSpace={workbench.activeSpace} onSelectSpace={actions.selectSpace} onCreateSpace={actions.promptCreateSpace} onOpenSettings={() => actions.setSettingsOpen(true)} />
      <StatusBar activeSpace={workbench.activeSpace} />
      <Toast />
      {workbench.settingsOpen ? <SettingsModal onClose={() => actions.setSettingsOpen(false)} onSignOut={actions.handleSignOut} activeSpace={workbench.activeSpace} /> : null}
      <DialogHost dialog={workbench.dialog} onClose={() => actions.setDialog(null)} />
    </div>
  );
}
