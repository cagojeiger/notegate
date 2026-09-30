import { FolderOpen } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import type { UpdateSpaceInput } from "../../api/spaces";
import type { Space } from "../../api/types";
import type { CurrentUserUsage } from "../../api/usage";
import { WORKBENCH_LAYOUT } from "../../shared/model/workbenchLayout";
import { Button, Card, Modal } from "../../shared/ui";
import { SortableSpaceGrid } from "./SortableSpaceGrid";
import { SpaceInspector, type SpaceInspectorProps } from "./SpaceInspector";
import { useReorderSpacesMutation, useUpdateSpaceMutation } from "./useSpaceQueries";
import { useCheckSpaceUsageMutation, useUsageQuery } from "./useUsageQueries";

type SpaceLibraryProps = {
  spaces: Space[];
  activeSpace: Space | null;
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
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-bg">
      <section className="flex min-w-0 flex-1 flex-col overflow-hidden">
        <header className="h-12 shrink-0 border-b border-seam px-5 sm:px-7 lg:px-10">
          <div className="flex h-full w-full items-center justify-between gap-3">
            <h1 className="text-xl font-semibold">
              Spaces <span className="font-normal text-muted">{spaces.length}</span>
            </h1>
            <Button onClick={onCreateSpace}>Create space</Button>
          </div>
        </header>

        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-6 sm:px-7 lg:px-10">
          <div className="w-full">
            {spaces.length === 0 ? (
              <Card className="grid min-h-56 place-items-center border-dashed text-center">
                <div>
                  <FolderOpen className="mx-auto text-muted" size={28} />
                  <h2 className="mt-3 font-semibold">No spaces yet</h2>
                  <p className="mt-1 text-sm text-muted">Create a space to start organizing your notes and files.</p>
                </div>
              </Card>
            ) : (
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
                />
              </section>
            )}
          </div>
        </div>
      </section>

      {!isMobile && inspectorOpen ? (
        <aside
          aria-label="Space inspector"
          className="flex h-full min-h-0 shrink-0 overflow-hidden"
          style={{ width: WORKBENCH_LAYOUT.defaultAuxiliaryWidth }}
        >
          <SpaceInspector {...inspectorProps} />
        </aside>
      ) : null}

      {isMobile && inspectorOpen && selectedSpace ? (
        <Modal
          title="Space Inspector"
          placement="bottom"
          width="max-w-none"
          onClose={onCloseInspector}
        >
          <SpaceInspector {...inspectorProps} showHeader={false} />
        </Modal>
      ) : null}
    </div>
  );
}

function usageState(query: { isLoading: boolean; isError: boolean; data?: CurrentUserUsage }): SpaceInspectorProps["usageState"] {
  if (query.isLoading) return "loading";
  if (query.isError) return "error";
  return "ready";
}
