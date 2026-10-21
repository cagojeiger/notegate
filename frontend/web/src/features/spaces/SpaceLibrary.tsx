import { useEffect, useMemo, useState } from "react";

import type { UpdateSpaceInput } from "../../api/spaces";
import type { Space } from "../../api/types";
import type { CurrentUserUsage } from "../../api/usage";
import { formatBytes } from "../../shared/lib/formatBytes";
import { Card, WorkbenchSurface } from "../../shared/ui";
import { SortableSpaceGrid } from "./SortableSpaceGrid";
import { SpaceInspector, type SpaceInspectorProps } from "./SpaceInspector";
import { useReorderSpacesMutation, useUpdateSpaceMutation } from "./useSpaceQueries";
import { useCheckSpaceUsageMutation, useUsageQuery } from "./useUsageQueries";

type SpaceLibraryProps = {
  spaces: Space[];
  activeSpace: Space | null;
  canCreateSpace: boolean;
  isMobile: boolean;
  usagePollingEnabled: boolean;
  inspectorOpen: boolean;
  onOpenInspector: () => void;
  onCloseInspector: () => void;
  onOpenSpace: (space: Space) => void;
  onCreateSpace: () => void;
};

export function SpaceLibrary({
  spaces,
  activeSpace,
  canCreateSpace,
  isMobile,
  usagePollingEnabled,
  inspectorOpen,
  onOpenInspector,
  onCloseInspector,
  onOpenSpace,
  onCreateSpace
}: SpaceLibraryProps) {
  const [selectedSpaceId, setSelectedSpaceId] = useState(activeSpace?.id ?? spaces[0]?.id ?? null);
  const usageQuery = useUsageQuery(usagePollingEnabled);
  const checkUsage = useCheckSpaceUsageMutation();
  const updateSpace = useUpdateSpaceMutation();
  const updateInspectorSpace = useUpdateSpaceMutation({ silentError: true });
  const reorderSpaces = useReorderSpacesMutation();
  const selectedSpace = spaces.find((space) => space.id === selectedSpaceId) ?? spaces[0] ?? null;
  const usageBySpaceId = useMemo(
    () => new Map((usageQuery.data?.spaces ?? []).map((usage) => [usage.id, usage])),
    [usageQuery.data?.spaces]
  );
  const selectedUsage = selectedSpace ? usageBySpaceId.get(selectedSpace.id) : undefined;
  const deletedStorage = usageQuery.data?.deleted_spaces;
  const currentUsageState = usageState(usageQuery);
  const updatePending = updateSpace.isPending || updateInspectorSpace.isPending;
  const selectedCheckError = checkUsage.isError && checkUsage.variables === selectedSpace?.id
    ? checkUsage.error
    : null;

  useEffect(() => {
    if (selectedSpaceId && spaces.some((space) => space.id === selectedSpaceId)) return;
    setSelectedSpaceId(activeSpace?.id ?? spaces[0]?.id ?? null);
  }, [activeSpace?.id, selectedSpaceId, spaces]);

  const updateSelectedSpace = (input: UpdateSpaceInput) => {
    if (!selectedSpace) return;
    updateInspectorSpace.mutate({ spaceId: selectedSpace.id, ...input });
  };
  const toggleNavigationPin = (space: Space) => {
    updateSpace.mutate({
      spaceId: space.id,
      navigation_pinned: !space.navigation_pinned
    });
  };
  const inspectSpace = (spaceId: string) => {
    setSelectedSpaceId(spaceId);
    onOpenInspector();
  };
  const inspectorProps: SpaceInspectorProps = {
    space: selectedSpace,
    usage: selectedUsage,
    usageState: currentUsageState,
    usageFetching: usageQuery.isFetching,
    pending: updatePending,
    error: updateInspectorSpace.isError,
    onRetryUsage: () => { void usageQuery.refetch(); },
    onUpdate: updateSelectedSpace,
    usageCheck: {
      disabled: checkUsage.isPending,
      error: selectedCheckError,
      hasRequested: checkUsage.variables === selectedSpace?.id && (checkUsage.isSuccess || checkUsage.isError),
      isRequesting: checkUsage.isPending && checkUsage.variables === selectedSpace?.id,
      onCheck: () => {
        if (!selectedSpace) return;
        checkUsage.reset();
        checkUsage.mutate(selectedSpace.id);
      }
    }
  };

  return (
    <WorkbenchSurface title={`Spaces ${spaces.length}`}
      tabs={[{ id: "spaces", label: "Spaces", count: spaces.length, controls: "workspace-spaces-panel" }]}
      value="spaces" onChange={() => undefined} navigationLabel="Workspace views" isMobile={isMobile}
      inspector={!isMobile || selectedSpace ? {
        title: "Space Inspector", label: "Space inspector", closeLabel: "Close space inspector",
        open: inspectorOpen, onClose: onCloseInspector, children: <SpaceInspector {...inspectorProps} showHeader={false} />
      } : undefined}>
      <div className="w-full">
        {spaces.length === 0 ? (
          <div className="mb-4">
            <h2 className="font-semibold">No spaces yet</h2>
            <p className="mt-1 text-sm text-muted">{canCreateSpace ? "Create a space to start organizing your notes and files." : "No spaces are available for this account."}</p>
          </div>
        ) : null}
        <section aria-label="Spaces">
          <SortableSpaceGrid
            spaces={spaces}
            selectedSpaceId={selectedSpace?.id ?? null}
            usageBySpaceId={usageBySpaceId}
            updatePending={updatePending || reorderSpaces.isPending}
            reorderPending={reorderSpaces.isPending || updatePending}
            onSelect={inspectSpace}
            onOpen={onOpenSpace}
            onToggleNavigationPin={toggleNavigationPin}
            onReorder={(orderedSpaces) => reorderSpaces.mutate({ spaces: orderedSpaces })}
            onCreateSpace={canCreateSpace ? onCreateSpace : undefined}
          />
        </section>
        {deletedStorage && deletedStorage.count > 0 ? (
          <section aria-label="Retained storage" className="mt-6 space-y-3">
            <h2 className="text-sm font-medium">Retained storage</h2>
            <Card className="space-y-2 p-4 text-xs">
              <p>{deletedStorage.count} deleted {deletedStorage.count === 1 ? "space" : "spaces"} still use storage while in trash or awaiting file deletion.</p>
              <p className="text-muted">Text {formatBytes(deletedStorage.text_bytes)} · Files {formatBytes(deletedStorage.file_bytes)}</p>
            </Card>
          </section>
        ) : null}
      </div>
    </WorkbenchSurface>
  );
}

function usageState(query: { isLoading: boolean; isError: boolean; data?: CurrentUserUsage }): SpaceInspectorProps["usageState"] {
  if (query.isLoading) return "loading";
  if (query.isError) return "error";
  return "ready";
}
