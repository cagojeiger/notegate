import { useState } from "react";

import { BrandLockup, Button, Card } from "../../shared/ui";
import { PublicInformationLinks } from "./PublicInformationLinks";

export function SignupPreview() {
  const [agreed, setAgreed] = useState(false);
  const [completed, setCompleted] = useState(false);

  return (
    <main className="ng-auth-screen grid h-full overflow-y-auto px-6 py-10 text-text">
      <section className="m-auto w-full max-w-[420px]">
        <header className="mb-7 text-center">
          <BrandLockup className="w-[190px]" />
        </header>

        <Card className="p-6 sm:p-7" aria-labelledby="signup-preview-title">
          <p className="mb-2 text-xs font-medium text-muted">Account setup preview</p>
          <h1 id="signup-preview-title" className="text-2xl font-semibold tracking-tight">Before you start</h1>
          <p id="signup-preview-notice" className="mt-4 rounded-xl border border-border bg-panel p-3 text-sm leading-6 text-muted">
            Preview only. Nothing is saved and no account is created.
          </p>
          <p className="mt-5 text-sm leading-6 text-muted">
            Review the <a href="/terms" target="_blank" rel="noopener noreferrer" className="rounded text-link underline underline-offset-4">Terms of Service</a> and <a href="/privacy" target="_blank" rel="noopener noreferrer" className="rounded text-link underline underline-offset-4">Privacy Policy</a>. Their content is still being prepared.
          </p>

          <form
            className="mt-5 space-y-4"
            onSubmit={(event) => {
              event.preventDefault();
              if (agreed) setCompleted(true);
            }}
          >
            <label className="flex min-h-11 cursor-pointer items-center gap-3 text-sm leading-6">
              <input
                type="checkbox"
                required
                checked={agreed}
                aria-describedby="signup-preview-notice"
                className="size-4 shrink-0 accent-primary"
                onChange={(event) => {
                  setAgreed(event.target.checked);
                  setCompleted(false);
                }}
              />
              <span>I agree to the Terms of Service.</span>
            </label>
            <Button type="submit" disabled={!agreed} className="h-11 w-full text-sm">Agree and continue</Button>
            {completed ? (
              <p role="status" className="rounded-xl border border-success/40 bg-success/10 p-3 text-sm leading-6 text-text">
                Preview complete. No agreement or account was saved.
              </p>
            ) : null}
          </form>

          <a href="/" className="mt-3 inline-flex min-h-11 w-full items-center justify-center rounded text-sm text-muted underline underline-offset-4 hover:text-text">
            Back to sign in
          </a>
        </Card>
        <footer className="mt-5"><PublicInformationLinks /></footer>
      </section>
    </main>
  );
}
