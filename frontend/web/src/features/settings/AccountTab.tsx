import type { Me } from "../../api/types";
import { Button, Card, SectionHeader } from "../../shared/ui";
import { AuthAccessManager } from "./AuthAccessManager";
import { SettingsInspectorSection } from "./SettingsInspector";
import { SettingsPage, type SettingsPageRenderer } from "./SettingsPage";

export function AccountTab({ me, onSignOut, renderPage }: {
  me: Me;
  onSignOut: () => void;
  renderPage: SettingsPageRenderer;
}) {
  const profile = <section>
    <SectionHeader title="Profile" />
    <Card className="break-words text-sm">
      <div className="font-medium">{me.account.display_name}</div>
      <div className="text-muted">{me.user?.email ?? me.account.kind}</div>
    </Card>
  </section>;

  if (me.account.kind === "user") return <AuthAccessManager kind="browser" onSignOut={onSignOut} summary={profile} renderPage={renderPage} />;
  const inspectorContents = <SettingsInspectorSection title="This browser">
    <p className="mb-3 text-xs text-muted">Signing out leaves connected applications and agent keys active.</p>
    <Button variant="danger" onClick={onSignOut}>Sign out</Button>
  </SettingsInspectorSection>;
  const body = <SettingsPage title="Account" inspectorLabel="Open account actions" scrollKey="account">
    {profile}
  </SettingsPage>;
  return renderPage(body, inspectorContents);
}
