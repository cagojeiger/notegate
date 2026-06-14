import { Button, Card, TextField } from "../shared/ui";
import { useDevAuthGateController, type DevAuthGateControllerProps } from "./useDevAuthGateController";

type DevAuthGateProps = DevAuthGateControllerProps;

export function DevAuthGate(props: DevAuthGateProps) {
  const auth = useDevAuthGateController(props);
  return (
    <main className="grid h-full place-items-center bg-bg px-6 text-text">
      <Card as="form" onSubmit={auth.handleSubmit} className="w-full max-w-md bg-panel p-6 shadow-[var(--ng-focus-shadow)]">
        <div className="mb-6 flex items-center gap-3">
          <div className="grid size-10 place-items-center rounded-xl bg-text text-lg font-bold text-bg">N</div>
          <div>
            <h1 className="text-xl font-semibold">Sign in to Notegate</h1>
            <p className="text-sm text-muted">Use your Notegate account to open the dashboard.</p>
          </div>
        </div>
        <Button className="w-full" onClick={auth.startLogin}>Continue with login</Button>
        <a
          href={auth.loginHref}
          target="notegate-login"
          onClick={() => auth.beginPolling(null)}
          className="mt-2 block text-center text-xs text-muted underline underline-offset-2 hover:text-text"
        >
          Or open the login page in a new window
        </a>
        <p className="mt-3 text-xs leading-5 text-muted">OAuth creates an HttpOnly browser session cookie.</p>
        {auth.loginHint ? <Card className="mt-3 text-xs leading-5 text-muted" padding="sm">{auth.loginHint}</Card> : null}
        <details className="mt-5 rounded-xl border border-border bg-surface p-3">
          <summary className="cursor-pointer text-sm font-medium">Developer API key fallback</summary>
          <TextField
            id="api-key"
            name="apiKey"
            label="User API key"
            value={auth.apiKey}
            onChange={(event) => auth.setApiKey(event.target.value)}
            inputClassName="mt-2 bg-bg font-mono text-sm"
            placeholder="ngk_v1_..."
            autoComplete="off"
          />
          <Button type="submit" className="mt-4 w-full" secondary disabled={!auth.canSubmitApiKey}>Open with API key</Button>
        </details>
      </Card>
    </main>
  );
}
