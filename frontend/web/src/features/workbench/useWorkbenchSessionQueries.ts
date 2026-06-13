import { useApiClient } from "../../api/ApiProvider";
import { logout } from "../../api/auth";

export function useLogout() {
  const client = useApiClient();
  return () => logout(client);
}
