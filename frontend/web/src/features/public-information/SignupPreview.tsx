import { useState } from "react";

import { AuthPage } from "../../auth/AuthPage";
import { Button, Notice } from "../../shared/ui";

export function SignupPreview() {
  const [agreed, setAgreed] = useState(false);
  const [completed, setCompleted] = useState(false);

  return (
    <AuthPage title="Before you start" footer={<a href="/">Back to sign in</a>}>
      <Notice id="signup-preview-notice" title="Account setup preview">
        <p>Preview only. Nothing is saved and no account is created.</p>
      </Notice>
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
        <Button type="submit" disabled={!agreed} className="auth-primary h-11 w-full text-sm">Agree and continue</Button>
        {completed ? (
          <Notice tone="success" role="status">
            <p>Preview complete. No agreement or account was saved.</p>
          </Notice>
        ) : null}
      </form>
    </AuthPage>
  );
}
