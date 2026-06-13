import { useEffect } from "react";

import { EditorArea } from "../features/editor/EditorArea";
import { PrimarySidebar } from "../features/nodes/PrimarySidebar";
import { ActivityRail } from "../features/spaces/ActivityRail";
import { MobileSpaceBar } from "../features/spaces/MobileSpaceBar";
import { useWorkbenchController } from "../features/workbench/useWorkbenchController";
import { useUiStore } from "../stores/uiStore";
import { AuxiliarySidebar } from "./AuxiliarySidebar";
import { DialogHost } from "./dialogs/DialogHost";
import { FullScreenStatus } from "./FullScreenStatus";
import { SettingsModal } from "./SettingsModal";
import { StatusBar } from "./StatusBar";
import { TitleBar } from "./TitleBar";

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
        <div
          style={workbench.isMobile ? undefined : { width: workbench.primaryWidth }}
          className={`min-h-0 max-md:fixed max-md:left-0 max-md:bottom-0 max-md:top-12 max-md:z-40 max-md:flex max-md:w-[85%] max-md:max-w-[320px] max-md:shadow-2xl max-md:transition-transform ${workbench.mobileTreeOpen ? "max-md:translate-x-0" : "max-md:-translate-x-full"} ${workbench.primarySidebarOpen ? "md:flex md:shrink-0" : "md:hidden"}`}
        >
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
        </div>
        {workbench.primarySidebarOpen ? (
          <div onPointerDown={actions.startPrimaryResize} className="hidden w-1 shrink-0 cursor-col-resize bg-seam transition-colors hover:bg-primary/40 md:block" aria-hidden="true" />
        ) : null}
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
        <div
          className={`min-h-0 hidden max-md:fixed max-md:inset-x-0 max-md:bottom-0 max-md:top-auto max-md:z-40 max-md:flex max-md:h-[70vh] max-md:max-w-none max-md:rounded-t-2xl max-md:shadow-2xl max-md:transition-transform ${workbench.mobileAuxOpen ? "max-md:translate-y-0" : "max-md:translate-y-full"} md:max-[1120px]:fixed md:max-[1120px]:right-0 md:max-[1120px]:top-12 md:max-[1120px]:bottom-7 md:max-[1120px]:z-30 md:max-[1120px]:w-[340px] md:max-[1120px]:shadow-2xl ${workbench.showAuxiliary ? "md:max-[1120px]:flex min-[1120px]:flex min-[1120px]:w-[320px] min-[1120px]:shrink-0" : "md:max-[1120px]:hidden min-[1120px]:hidden"}`}
        >
          <AuxiliarySidebar activeNode={workbench.activeNode} onReplaceMetadata={actions.promptReplaceMetadata} />
        </div>
        {workbench.mobileTreeOpen || workbench.mobileAuxOpen ? (
          <button type="button" aria-label="Close panel" onClick={actions.closeMobile} className="fixed inset-x-0 bottom-0 top-12 z-30 bg-black/40 md:hidden" />
        ) : null}
      </main>
      <MobileSpaceBar spaces={workbench.spaces} activeSpace={workbench.activeSpace} onSelectSpace={actions.selectSpace} onCreateSpace={actions.promptCreateSpace} onOpenSettings={() => actions.setSettingsOpen(true)} />
      <StatusBar activeSpace={workbench.activeSpace} />
      <Toast />
      {workbench.settingsOpen ? <SettingsModal onClose={() => actions.setSettingsOpen(false)} onSignOut={actions.handleSignOut} activeSpace={workbench.activeSpace} /> : null}
      <DialogHost dialog={workbench.dialog} onClose={() => actions.setDialog(null)} />
    </div>
  );
}

function Toast() {
  const toast = useUiStore((state) => state.toast);
  const clearToast = useUiStore((state) => state.clearToast);
  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(clearToast, 2000);
    return () => window.clearTimeout(timer);
  }, [toast, clearToast]);
  if (!toast) return null;
  return (
    <div className="pointer-events-none fixed inset-x-0 bottom-20 z-50 flex justify-center md:bottom-10">
      <div className="rounded-full border border-border bg-panel-strong px-4 py-2 text-sm text-text shadow-lg">{toast}</div>
    </div>
  );
}
