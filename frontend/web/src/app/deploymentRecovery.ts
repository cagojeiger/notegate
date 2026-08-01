export const DEPLOYMENT_RELOAD_PROMPT = "NoteGate couldn't load this screen. Reload the app to get the latest version? Unsaved changes may be lost.";

export function handleDeploymentPreloadError(
  event: Event,
  confirmReload: (message: string) => boolean,
  reload: () => void
) {
  event.preventDefault();
  if (confirmReload(DEPLOYMENT_RELOAD_PROMPT)) reload();
}

export function installDeploymentRecovery() {
  window.addEventListener("vite:preloadError", (event) => {
    handleDeploymentPreloadError(
      event,
      (message) => window.confirm(message),
      () => window.location.reload()
    );
  });
}
