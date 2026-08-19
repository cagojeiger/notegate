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
    <span
      role={decorative ? undefined : "img"}
      aria-label={decorative ? undefined : "NoteGate"}
      aria-hidden={decorative || undefined}
      className={cn("grid shrink-0", className)}
      style={{ width: size, height: size }}
    >
      <img
        src="/brand/source/app-icon-light.svg"
        width={size}
        height={size}
        alt=""
        aria-hidden="true"
        className="ng-brand-theme-light col-start-1 row-start-1 h-full w-full"
      />
      <img
        src="/brand/source/app-icon-dark.svg"
        width={size}
        height={size}
        alt=""
        aria-hidden="true"
        className="ng-brand-theme-dark col-start-1 row-start-1 h-full w-full"
      />
    </span>
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
