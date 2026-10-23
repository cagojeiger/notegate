export function PublicInformationLinks() {
  return (
    <nav aria-label="Legal and support" className="flex flex-wrap justify-center gap-x-3 text-xs text-muted">
      <a href="/terms" className="inline-flex min-h-[var(--ng-min-target-size)] items-center rounded px-2 underline underline-offset-4 hover:text-text">Terms</a>
      <a href="/privacy" className="inline-flex min-h-[var(--ng-min-target-size)] items-center rounded px-2 underline underline-offset-4 hover:text-text">Privacy</a>
      <a href="/support" className="inline-flex min-h-[var(--ng-min-target-size)] items-center rounded px-2 underline underline-offset-4 hover:text-text">Support</a>
    </nav>
  );
}
