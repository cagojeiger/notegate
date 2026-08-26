import type { InfiniteData, UseInfiniteQueryResult } from "@tanstack/react-query";
import { History, RefreshCw } from "lucide-react";

import type { AuditEventListResponse, BackgroundJobListResponse, CommandInvocationListResponse, FileChangeEventListResponse } from "../../api/types";
import { Button, EmptyState } from "../../shared/ui";
import { formatEventTime, formatEventTimeCompact } from "./eventDisplay";

export type EventListResponse = AuditEventListResponse | BackgroundJobListResponse | CommandInvocationListResponse | FileChangeEventListResponse;
export type EventHistoryQuery<T extends EventListResponse> = UseInfiniteQueryResult<InfiniteData<T, unknown>, Error>;

export function RefreshButton({
  isFetching,
  onRefresh,
  disabled = false
}: {
  isFetching: boolean;
  onRefresh: () => void;
  disabled?: boolean;
}) {
  return (
    <Button size="sm" secondary onClick={onRefresh} disabled={disabled || isFetching}>
      <RefreshCw size={14} className={isFetching ? "animate-spin" : ""} /> Refresh
    </Button>
  );
}

export function EventQueryState<T extends EventListResponse>({ query, itemCount, emptyLabel }: { query: EventHistoryQuery<T>; itemCount: number; emptyLabel: string }) {
  if (query.isLoading) return <div className="text-workbench text-muted">Loading…</div>;
  if (query.isError) return <EmptyState>Could not load history.</EmptyState>;
  if (itemCount === 0) return <EmptyState>{emptyLabel}</EmptyState>;
  return null;
}

export function EventTime({ value }: { value: string }) {
  return (
    <time className="text-xs text-muted" dateTime={value}>
      <History size={14} className="mr-1 inline-block align-[-2px]" />
      <span className="sm:hidden">{formatEventTimeCompact(value)}</span>
      <span className="hidden sm:inline">{formatEventTime(value)}</span>
    </time>
  );
}

export function LifecycleTime({ label, value }: { label: string; value: string }) {
  const full = formatEventTime(value);
  return (
    <time dateTime={value} aria-label={`${label} ${full}`} title={`${label} ${full}`}>
      <span>{label} </span>
      <span className="sm:hidden">{formatEventTimeCompact(value)}</span>
      <span className="hidden sm:inline">{full}</span>
    </time>
  );
}

export function LoadMore<T extends EventListResponse>({ query }: { query: EventHistoryQuery<T> }) {
  if (!query.hasNextPage) return null;
  return (
    <div className="flex justify-center">
      <Button size="sm" secondary onClick={() => { void query.fetchNextPage(); }} disabled={query.isFetchingNextPage}>
        {query.isFetchingNextPage ? "Loading…" : "Load more"}
      </Button>
    </div>
  );
}
