import { useUiStore } from "../stores/uiStore";
import { clearPersistedWorkbenches } from "../stores/workbenchStorage";
import { clearDevApiKey } from "../auth/session";

export function resetWorkbenchClientState(): void {
  clearPersistedWorkbenches();
  useUiStore.getState().resetWorkbenchSession();
}

export function clearAuthenticatedClientState(): void {
  clearDevApiKey();
  resetWorkbenchClientState();
}
