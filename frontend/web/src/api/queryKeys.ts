import type { FilePreviewKind } from "./types";

export const queryKeys = {
  me: ["me"] as const,
  usage: ["me", "usage"] as const,
  auditEvents: ["me", "audit-events"] as const,
  mcpInvocations: ["me", "mcp-invocations"] as const,
  backgroundJobs: ["me", "jobs"] as const,
  backgroundJob: (jobId: string) => ["me", "jobs", jobId] as const,
  agents: ["agents"] as const,
  agentKeys: (agentId: string) => ["agents", agentId, "keys"] as const,
  connections: (spaceId: string) => ["spaces", spaceId, "connections"] as const,
  spaces: ["spaces"] as const,
  space: (spaceId: string) => ["spaces", spaceId] as const,
  childrenFamily: (spaceId: string) => ["spaces", spaceId, "children"] as const,
  childrenRevision: (spaceId: string) => ["spaces", spaceId, "children-revision"] as const,
  spaceChangeSignal: (spaceId: string) => ["sync", "space-change", spaceId] as const,
  treeRestore: (
    spaceId: string,
    attemptKey: string,
    parentIds: readonly string[]
  ) => ["tree-restore", spaceId, attemptKey, parentIds] as const,
  fileChangeEventsFamily: (spaceId: string) => ["spaces", spaceId, "file-change-events"] as const,
  fileChangeEvents: (spaceId: string, nodeId?: string | null) => ["spaces", spaceId, "file-change-events", nodeId ?? "space"] as const,
  children: (spaceId: string, nodeId: string) => ["spaces", spaceId, "children", nodeId] as const,
  recent: (spaceId: string) => ["spaces", spaceId, "recent"] as const,
  nodes: (spaceId: string) => ["spaces", spaceId, "nodes"] as const,
  node: (spaceId: string, nodeId: string) => ["spaces", spaceId, "nodes", nodeId] as const,
  texts: (spaceId: string) => ["spaces", spaceId, "text"] as const,
  text: (spaceId: string, nodeId: string) => ["spaces", spaceId, "text", nodeId] as const,
  files: (spaceId: string) => ["spaces", spaceId, "file"] as const,
  file: (spaceId: string, nodeId: string) => ["spaces", spaceId, "file", nodeId] as const,
  markdownImagePreviews: (spaceId: string) => ["spaces", spaceId, "markdown-image-preview"] as const,
  markdownImagePreview: (spaceId: string, path: string) => ["spaces", spaceId, "markdown-image-preview", path] as const,
  filePreviewUrls: (spaceId: string) => ["file-preview-urls", spaceId] as const,
  filePreviewNode: (spaceId: string, nodeId: string) => ["file-preview-urls", spaceId, nodeId] as const,
  filePreviewUrl: (spaceId: string, nodeId: string, kind: FilePreviewKind) => (
    ["file-preview-urls", spaceId, nodeId, kind] as const
  ),
  audioPreviewUrl: (spaceId: string, nodeId: string) => (
    ["file-preview-urls", spaceId, nodeId, "audio"] as const
  )
};
