import { ExternalLink, RotateCcw } from "lucide-react";

import { Button, Card, SectionHeader } from "../../shared/ui";

const appVersion = import.meta.env.VITE_NOTEGATE_VERSION || "development";

export function GeneralTab({ onResetSavedWorkspace }: { onResetSavedWorkspace: () => void }) {
  return (
    <div className="space-y-4">
      <section>
        <SectionHeader title="Saved workspace" description="Open panes and panel visibility are restored on this browser." />
        <Card className="flex items-start justify-between gap-4 text-sm">
          <div className="min-w-0">
            <div className="font-medium">Saved open panes</div>
            <p className="mt-1 max-w-md text-xs leading-5 text-muted">Reset the browser-only pane snapshots and panel visibility used when returning to a space or refreshing NoteGate.</p>
          </div>
          <Button variant="danger" className="shrink-0" onClick={onResetSavedWorkspace}>
            <RotateCcw size={15} />
            Reset
          </Button>
        </Card>
      </section>

      <section>
        <SectionHeader title="About" />
        <Card className="flex items-center gap-4 text-sm">
          <span className="min-w-0 flex-1 font-medium">NoteGate</span>
          <span aria-label={`Version v${appVersion}`} className="text-muted">Version <code className="ml-1 font-mono text-text">v{appVersion}</code></span>
          <a
            href="https://github.com/cagojeiger/notegate"
            target="_blank"
            rel="noopener noreferrer"
            aria-label="Open NoteGate on GitHub"
            title="Open NoteGate on GitHub"
            className="grid size-8 shrink-0 place-items-center rounded-[10px] text-muted outline-none transition hover:bg-[var(--ng-hover)] hover:text-text focus-visible:ring-2 focus-visible:ring-primary/45"
          >
            <ExternalLink size={17} />
          </a>
        </Card>
      </section>
    </div>
  );
}
