import { logout } from "../../api/auth";

export function useLogout() {
  return () => logout();
}
