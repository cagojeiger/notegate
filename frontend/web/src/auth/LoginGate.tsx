import { Notice } from "../shared/ui";
import { AuthPage } from "./AuthPage";
import { useLoginGateController, type LoginGateControllerProps } from "./useLoginGateController";

export function LoginGate(controllerProps: LoginGateControllerProps) {
  const auth = useLoginGateController(controllerProps);

  return (
    <AuthPage
      title="Continue to NoteGate"
      footer={<a href="/signup/preview">Preview account setup</a>}
    >
      <p className="mb-6 text-sm leading-6 text-muted">Sign in with your connected Google account to open your Spaces.</p>
      <button type="button" className="ng-google-button" onClick={auth.startLogin}>
        <img src="/google-g.png" className="size-5 object-contain" alt="" aria-hidden="true" />
        Continue with Google
      </button>
      <a
        href={auth.loginHref}
        target="notegate-login"
        onClick={() => auth.beginPolling(null)}
        className="mt-3 block rounded text-center text-xs underline underline-offset-2"
      >
        Open Google sign-in in a new window
      </a>
      <Notice className="mt-6" title="About sign-in"><p>Your Google password stays with Google.</p></Notice>
      {auth.loginHint ? <Notice className="mt-3" role="status" aria-live="polite">{auth.loginHint}</Notice> : null}
    </AuthPage>
  );
}
