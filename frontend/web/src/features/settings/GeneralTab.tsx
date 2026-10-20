import { ExternalLink, Moon, RotateCcw, Sun } from "lucide-react";

import { Button, Card, SectionHeader } from "../../shared/ui";
import { useUiStore } from "../../stores/uiStore";
import { SettingsControls, SettingsInspectorSection } from "./SettingsInspector";

import { SettingsPage } from "./SettingsPage";

const appVersion = import.meta.env.VITE_NOTEGATE_VERSION || "development";

export function GeneralTab({ onResetSavedWorkspace }: { onResetSavedWorkspace: () => void }) {
  const theme = useUiStore((state) => state.theme);
  const toggleTheme = useUiStore((state) => state.toggleTheme);
  return (
    <SettingsPage title="General" inspectorLabel="Open general actions" scrollKey="general">
      <div className="ng-settings-grid">
        <section>
          <SectionHeader title="About" />
          <Card className="flex flex-wrap items-center gap-4 text-sm">
            <span className="min-w-0 flex-1 font-medium">NoteGate</span>
            <span aria-label={`Version v${appVersion}`} className="text-muted">Version <code className="ml-1 font-mono text-text">v{appVersion}</code></span>
            <a href="https://github.com/cagojeiger/notegate" target="_blank" rel="noopener noreferrer"
              aria-label="Open NoteGate on GitHub" title="Open NoteGate on GitHub"
              className="grid size-8 shrink-0 place-items-center rounded-workbench-surface text-muted outline-none transition hover:bg-[var(--ng-hover)] hover:text-text focus-visible:ring-2 focus-visible:ring-primary/45">
              <ExternalLink size={17} />
            </a>
          </Card>
        </section>
        <section>
          <SectionHeader title="Appearance" />
          <button type="button" onClick={toggleTheme} className="flex min-h-16 w-full items-center justify-between gap-4 rounded-workbench-surface border border-border bg-surface p-4 text-workbench transition hover:bg-panel">
            <span>Theme</span><span className="flex items-center gap-2 capitalize text-muted">{theme === "light" ? <Sun size={16} /> : <Moon size={16} />} {theme}</span>
          </button>
        </section>
        <section className="ng-settings-wide">
          <SectionHeader title="Saved workspace" description="Open panes and panel visibility are restored on this browser." />
          <Card className="text-sm">
            <div className="font-medium">Saved open panes</div>
            <p className="mt-1 text-xs leading-5 text-muted">Reset the browser-only pane snapshots and panel visibility used when returning to a space or refreshing NoteGate. Your spaces, documents and files are kept.</p>
          </Card>
        </section>
        <SettingsControls>
          <SettingsInspectorSection title="Workspace actions">
            <p className="mb-3 text-xs text-muted">Only this browser's saved layout is affected.</p>
            <Button secondary onClick={onResetSavedWorkspace}><RotateCcw size={15} /> Reset</Button>
          </SettingsInspectorSection>
        </SettingsControls>
      </div>
    </SettingsPage>
  );
}
