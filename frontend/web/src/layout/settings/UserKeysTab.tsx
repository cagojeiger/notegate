import { useApiClient } from "../../api/ApiProvider";
import { createMyKey, listMyKeys, revokeMyKey } from "../../api/keys";
import { queryKeys } from "../../api/queryKeys";
import { KeyManager } from "./KeyManager";

export function UserKeysTab() {
  const client = useApiClient();
  return <KeyManager queryKey={queryKeys.myKeys} list={() => listMyKeys(client)} create={(input) => createMyKey(client, input)} revoke={(id) => revokeMyKey(client, id)} emptyLabel="No active keys." />;
}
