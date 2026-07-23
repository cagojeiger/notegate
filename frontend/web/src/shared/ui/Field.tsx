import type { InputHTMLAttributes, ReactNode, SelectHTMLAttributes, TextareaHTMLAttributes } from "react";

import { cn } from "../lib/cn";

function FieldShell({ label, children, className }: { label: string; children: ReactNode; className?: string }) {
  return (
    <label className={cn("block min-w-0 text-sm", className)}>
      <span className="mb-1.5 block text-xs text-muted">{label}</span>
      {children}
    </label>
  );
}

const controlClass = "w-full rounded-lg border border-border-strong bg-surface px-3 py-2 text-text outline-none transition placeholder:text-faint disabled:cursor-not-allowed disabled:opacity-50";

export function TextField({ label, className, inputClassName, ...props }: InputHTMLAttributes<HTMLInputElement> & { label: string; inputClassName?: string }) {
  return (
    <FieldShell label={label} className={className}>
      <input className={cn(controlClass, inputClassName)} {...props} />
    </FieldShell>
  );
}

export function TextAreaField({ label, className, textareaClassName, ...props }: TextareaHTMLAttributes<HTMLTextAreaElement> & { label: string; textareaClassName?: string }) {
  return (
    <FieldShell label={label} className={className}>
      <textarea className={cn(controlClass, "min-h-24 resize-y", textareaClassName)} {...props} />
    </FieldShell>
  );
}

export function SelectField({ label, className, children, ...props }: SelectHTMLAttributes<HTMLSelectElement> & { label: string; children: ReactNode }) {
  return (
    <FieldShell label={label} className={className}>
      <select className={controlClass} {...props}>
        {children}
      </select>
    </FieldShell>
  );
}
