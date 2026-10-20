import type { Me } from "../../api/types";
import { Button, Card, SectionHeader } from "../../shared/ui";
import { AuthAccessManager } from "./AuthAccessManager";
import { SettingsControls, SettingsInspectorSection } from "./SettingsInspector";
import { SettingsPage } from "./SettingsPage";

export function AccountTab({ me, onSignOut }: { me: Me; onSignOut: () => void }) {
  const profile = <section>
    <SectionHeader title="Profile" />
    <Card className="text-sm">
      <div className="font-medium">{me.account.display_name}</div>
      <div className="text-muted">{me.user?.email ?? me.account.kind}</div>
    </Card>
  </section>;

  if (me.account.kind === "user") return <AuthAccessManager kind="browser" onSignOut={onSignOut} summary={profile} />;
  return <SettingsPage title="Account" inspectorLabel="Open account actions" scrollKey="account">
    {profile}
    <SettingsControls><SettingsInspectorSection title="This browser">
      <p className="mb-3 text-xs text-muted">Signing out leaves connected applications and agent keys active.</p>
      <Button variant="danger" onClick={onSignOut}>Sign out</Button>
    </SettingsInspectorSection></SettingsControls>
  </SettingsPage>;
}
