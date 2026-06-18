import { RotateCcw } from "lucide-react";

import { Button, Card, SectionHeader } from "../../shared/ui";

export function GeneralTab({ onResetSavedWorkspace }: { onResetSavedWorkspace: () => void }) {
  return (
    <div className="space-y-4">
      <section>
        <SectionHeader title="Saved workspace" description="Open panes are restored per space on this browser." />
        <Card className="flex items-start justify-between gap-4 text-sm">
          <div className="min-w-0">
            <div className="font-medium">Saved open panes</div>
            <p className="mt-1 max-w-md text-xs leading-5 text-muted">Reset the browser-only pane snapshots used when returning to a space or refreshing Notegate.</p>
          </div>
          <Button variant="danger" className="shrink-0" onClick={onResetSavedWorkspace}>
            <RotateCcw size={15} />
            Reset
          </Button>
        </Card>
      </section>
    </div>
  );
}
