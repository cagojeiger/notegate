import { Moon, Sun } from "lucide-react";

import type { Me } from "../../api/types";
import { Button, Card, SectionHeader } from "../../shared/ui";
import { KeyManager } from "./KeyManager";
import { useUiStore } from "../../stores/uiStore";
import { useMyKeyManagerProps } from "./useSettingsQueries";

export function AccountTab({ me, onSignOut }: { me: Me | undefined; onSignOut: () => void }) {
  const theme = useUiStore((state) => state.theme);
  const toggleTheme = useUiStore((state) => state.toggleTheme);
  const keyManagerProps = useMyKeyManagerProps();
  return (
    <div className="space-y-4">
      <section>
        <SectionHeader title="Account" />
        <Card className="text-sm">
          <div className="font-medium">{me?.account.display_name ?? "…"}</div>
          <div className="text-muted">{me?.user?.email ?? me?.account.kind ?? ""}</div>
        </Card>
      </section>

      <section>
        <SectionHeader title="Appearance" />
        <button type="button" onClick={toggleTheme} className="flex w-full items-center justify-between rounded-xl border border-border bg-surface p-4 text-sm transition hover:bg-panel">
          <span>Theme</span>
          <span className="flex items-center gap-2 capitalize text-muted">{theme === "light" ? <Sun size={16} /> : <Moon size={16} />} {theme}</span>
        </button>
      </section>

      <section>
        <SectionHeader title="My API Keys" description="User keys authenticate as your account." />
        <KeyManager {...keyManagerProps} emptyLabel="No user API keys." />
      </section>

      <Button variant="danger" className="w-full" onClick={onSignOut}>Sign out</Button>
    </div>
  );
}
