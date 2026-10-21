import type { InputHTMLAttributes, ReactNode, SelectHTMLAttributes, TextareaHTMLAttributes } from "react";

import { cn } from "../lib/cn";

function FieldShell({ label, children, className }: { label: string; children: ReactNode; className?: string }) {
  return (
    <label className={cn("block min-w-0 font-ui text-workbench", className)}>
      <span className="mb-1.5 block text-xs text-muted">{label}</span>
      {children}
    </label>
  );
}

const controlClass = "w-full rounded-workbench-surface border border-border-strong bg-surface px-2.5 py-1 font-ui text-workbench text-text outline-none transition placeholder:text-faint focus-visible:ring-2 focus-visible:ring-primary/45 disabled:cursor-not-allowed disabled:opacity-50";

export function TextField({ label, className, inputClassName, ...props }: InputHTMLAttributes<HTMLInputElement> & { label: string; inputClassName?: string }) {
  return (
    <FieldShell label={label} className={className}>
      <input className={cn(controlClass, "min-h-workbench-control", inputClassName)} {...props} />
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
      <Select {...props}>
        {children}
      </Select>
    </FieldShell>
  );
}

export function Select({ className, ...props }: SelectHTMLAttributes<HTMLSelectElement>) {
  return <select className={cn(controlClass, "min-h-workbench-control", className)} {...props} />;
}
