import { useInfiniteQuery, useQuery } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import { getBackgroundJob, listAuditEvents, listBackgroundJobs, listCommandInvocations, listFileChangeEvents } from "../../api/events";
import { queryKeys } from "../../api/queryKeys";
import type { CommandInvocationSurface } from "../../api/types";

export function useAuditEventsQuery() {
  const client = useApiClient();
  return useInfiniteQuery({
    queryKey: queryKeys.auditEvents,
    queryFn: ({ pageParam }) => listAuditEvents(client, pageParam),
    initialPageParam: null as string | null,
    getNextPageParam: (lastPage) => (lastPage.page.has_more ? lastPage.page.next_cursor : undefined)
  });
}

export function useCommandInvocationsQuery(surface: CommandInvocationSurface) {
  const client = useApiClient();
  return useInfiniteQuery({
    queryKey: queryKeys.commandInvocations(surface),
    queryFn: ({ pageParam }) => listCommandInvocations(client, surface, pageParam),
    initialPageParam: null as string | null,
    getNextPageParam: (lastPage) => (lastPage.page.has_more ? lastPage.page.next_cursor : undefined)
  });
}

export function useBackgroundJobsQuery() {
  const client = useApiClient();
  return useInfiniteQuery({
    queryKey: queryKeys.backgroundJobs,
    queryFn: ({ pageParam }) => listBackgroundJobs(client, pageParam),
    initialPageParam: null as string | null,
    getNextPageParam: (lastPage) => (lastPage.page.has_more ? lastPage.page.next_cursor : undefined),
    refetchInterval: (query) => query.state.data?.pages.some((page) => (
      page.jobs.some((job) => job.status === "queued" || job.status === "running")
    )) ? 2_000 : false
  });
}

export function useBackgroundJobQuery(jobId: string, enabled: boolean) {
  const client = useApiClient();
  return useQuery({
    queryKey: queryKeys.backgroundJob(jobId),
    queryFn: () => getBackgroundJob(client, jobId),
    enabled,
    refetchInterval: (query) => {
      const status = query.state.data?.job.status;
      return status === "queued" || status === "running" ? 2_000 : false;
    }
  });
}

export function useFileChangeEventsQuery(spaceId: string | null, nodeId: string | null) {
  const client = useApiClient();
  return useInfiniteQuery({
    queryKey: spaceId ? queryKeys.fileChangeEvents(spaceId, nodeId) : queryKeys.fileChangeEvents("none", nodeId),
    queryFn: ({ pageParam }) => {
      if (!spaceId) throw new Error("Space is required");
      return listFileChangeEvents(client, spaceId, { nodeId, cursor: pageParam });
    },
    initialPageParam: null as string | null,
    getNextPageParam: (lastPage) => (lastPage.page.has_more ? lastPage.page.next_cursor : undefined),
    enabled: Boolean(spaceId)
  });
}
