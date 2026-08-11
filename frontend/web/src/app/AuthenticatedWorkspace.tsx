import type { Me } from "../api/types";
import { AudioRecordingProvider } from "../features/recording/AudioRecordingProvider";
import { UploadProvider } from "../features/uploads/UploadProvider";
import { AppShell } from "../layout/AppShell";

export default function AuthenticatedWorkspace({
  me,
  onSignOut
}: {
  me: Me;
  onSignOut: () => void;
}) {
  return (
    <UploadProvider>
      <AudioRecordingProvider>
        <AppShell me={me} onSignOut={onSignOut} />
      </AudioRecordingProvider>
    </UploadProvider>
  );
}
