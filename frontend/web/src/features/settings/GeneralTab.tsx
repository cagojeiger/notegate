import { RotateCcw } from "lucide-react";

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
        <Card className="flex items-center justify-between gap-4 text-sm">
          <span className="font-medium">NoteGate</span>
          <span className="text-muted">Version <code className="ml-1 font-mono text-text">v{appVersion}</code></span>
        </Card>
      </section>
    </div>
  );
}
