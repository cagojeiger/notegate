import { ExternalLink, Moon, RotateCcw, Sun } from "lucide-react";

import { Button, Card, SectionHeader } from "../../shared/ui";
import { useUiStore } from "../../stores/uiStore";

import { SettingsPage } from "./SettingsPage";

const appVersion = import.meta.env.VITE_NOTEGATE_VERSION || "development";

export function GeneralTab({ onResetSavedWorkspace }: { onResetSavedWorkspace: () => void }) {
  const theme = useUiStore((state) => state.theme);
  const toggleTheme = useUiStore((state) => state.toggleTheme);
  return (
    <SettingsPage title="General" scrollKey="general">
      <div className="ng-settings-grid">
        <section>
          <SectionHeader title="About" />
          <Card padding="sm" className="flex flex-wrap items-center gap-3 text-workbench">
            <span className="min-w-0 flex-1 font-medium">NoteGate</span>
            <span aria-label={`Version v${appVersion}`} className="text-muted">Version <code className="ml-1 font-mono text-text">v{appVersion}</code></span>
            <a href="https://github.com/cagojeiger/notegate" target="_blank" rel="noopener noreferrer"
              aria-label="Open NoteGate on GitHub" title="Open NoteGate on GitHub"
              className="grid size-workbench-control shrink-0 place-items-center rounded-workbench-surface text-muted outline-none transition hover:bg-[var(--ng-hover)] hover:text-text">
              <ExternalLink size={17} />
            </a>
          </Card>
        </section>
        <section>
          <SectionHeader title="Appearance" />
          <Card padding="sm" className="flex flex-wrap items-center gap-3 text-workbench">
            <span className="min-w-0 flex-1">Theme</span>
            <Button secondary onClick={toggleTheme} aria-label={`Switch to ${theme === "light" ? "dark" : "light"} theme`}>
              {theme === "light" ? <Sun size={16} /> : <Moon size={16} />} <span className="capitalize">{theme}</span>
            </Button>
          </Card>
        </section>
        <section className="ng-settings-wide">
          <SectionHeader title="Saved workspace" description="Open panes and panel visibility are restored on this browser." />
          <Card padding="sm" className="flex flex-wrap items-center gap-3 text-workbench">
            <div className="min-w-0 flex-1 basis-64">
              <div className="font-medium">Saved open panes</div>
              <p className="mt-1 text-xs leading-5 text-muted">Reset the browser-only pane snapshots and panel visibility used when returning to a space or refreshing NoteGate. Your spaces, documents and files are kept.</p>
            </div>
            <Button secondary onClick={onResetSavedWorkspace}><RotateCcw size={15} /> Reset</Button>
          </Card>
        </section>
      </div>
    </SettingsPage>
  );
}
