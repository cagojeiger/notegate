import type { HTMLAttributes } from "react";

import { cn } from "../lib/cn";

export function Notice({
  title,
  tone = "neutral",
  className,
  children,
  ...props
}: HTMLAttributes<HTMLDivElement> & { tone?: "neutral" | "success" | "danger" }) {
  return (
    <div className={cn("auth-note", tone !== "neutral" && `auth-note--${tone}`, className)} {...props}>
      {title ? <strong className="auth-note-title">{title}</strong> : null}
      {children}
    </div>
  );
}
