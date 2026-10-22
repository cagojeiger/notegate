import { Copy } from "lucide-react";

import { copyText } from "../../shared/lib/clipboard";
import { IconButton } from "../../shared/ui";
import { useUiStore } from "../../stores/uiStore";

export function EndpointRow({ label, value, copyLabel }: { label: string; value: string; copyLabel: string }) {
  const showToast = useUiStore((state) => state.showToast);

  async function copy() {
    showToast((await copyText(value)) ? "Copied" : "Could not copy");
  }

  return (
    <div>
      <div className="mb-1 text-xs font-semibold uppercase tracking-[0.16em] text-muted">{label}</div>
      <div className="flex h-workbench-control items-center rounded-workbench-surface bg-bg shadow-[inset_0_0_0_1px_var(--ng-border-strong)]">
        <code className="flex min-w-0 flex-1 items-center self-stretch border-r border-border-strong px-3 font-mono text-xs"><span className="truncate">{value}</span></code>
        <IconButton label={copyLabel} onClick={() => { void copy(); }}><Copy size={16} /></IconButton>
      </div>
    </div>
  );
}
