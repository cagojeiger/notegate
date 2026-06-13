import { KeyManager } from "./KeyManager";
import { useMyKeyManagerProps } from "./useSettingsQueries";

export function UserKeysTab() {
  const keyManagerProps = useMyKeyManagerProps();
  return <KeyManager {...keyManagerProps} emptyLabel="No active keys." />;
}
