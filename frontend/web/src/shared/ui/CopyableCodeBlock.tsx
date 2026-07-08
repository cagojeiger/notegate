import { Check, Copy } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";

import { copyText } from "../lib/clipboard";
import { Button } from "./Button";

export function CopyableCodeBlock({ code, label, children }: { code: string; label: string; children: ReactNode }) {
  const [status, setStatus] = useState<"idle" | "copied" | "failed">("idle");

  useEffect(() => {
    if (status === "idle") return;
    const timeout = window.setTimeout(() => setStatus("idle"), 1500);
    return () => window.clearTimeout(timeout);
  }, [status]);

  async function handleCopy() {
    setStatus((await copyText(code)) ? "copied" : "failed");
  }

  const ariaLabel = status === "copied" ? "Copied code" : status === "failed" ? "Could not copy code" : "Copy code";
  const buttonText = status === "copied" ? "Copied" : status === "failed" ? "Failed" : "Copy";
  const liveStatus = status === "copied" ? "Copied code" : status === "failed" ? "Could not copy code" : "";

  return (
    <div className="ng-code-copy" data-copy-status={status}>
      <div className="ng-code-copy-header">
        <span className="ng-code-copy-title">{label}</span>
        <Button aria-label={ariaLabel} className="ng-code-copy-button" size="xs" variant="secondary" onClick={() => { void handleCopy(); }}>
          {status === "copied" ? <Check size={14} /> : <Copy size={14} />}
          <span>{buttonText}</span>
        </Button>
      </div>
      <span className="ng-code-copy-status" role="status" aria-live="polite">{liveStatus}</span>
      {children}
    </div>
  );
}
