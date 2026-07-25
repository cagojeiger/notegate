import { cn } from "../lib/cn";

export function BrandAppIcon({
  size = 32,
  className,
  decorative = false
}: {
  size?: number;
  className?: string;
  decorative?: boolean;
}) {
  return (
    <img
      src="/brand/source/app-icon.svg"
      width={size}
      height={size}
      alt={decorative ? "" : "NoteGate"}
      aria-hidden={decorative || undefined}
      className={cn("shrink-0", className)}
    />
  );
}

export function BrandLockup({ className }: { className?: string }) {
  return (
    <span className={cn("inline-flex", className)}>
      <img
        src="/brand/svg/logo-horizontal-light.svg"
        width="190"
        height="40"
        alt="NoteGate"
        className="ng-brand-theme-light h-auto w-full"
      />
      <img
        src="/brand/svg/logo-horizontal-dark.svg"
        width="190"
        height="40"
        alt="NoteGate"
        className="ng-brand-theme-dark h-auto w-full"
      />
    </span>
  );
}
