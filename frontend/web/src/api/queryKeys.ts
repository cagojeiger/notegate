export const queryKeys = {
  me: ["me"] as const,
  myKeys: ["me", "keys"] as const,
  spaces: ["spaces"] as const,
  children: (spaceId: string, nodeId: string) => ["spaces", spaceId, "children", nodeId] as const,
  recent: (spaceId: string) => ["spaces", spaceId, "recent"] as const,
  node: (spaceId: string, nodeId: string) => ["spaces", spaceId, "nodes", nodeId] as const,
  text: (spaceId: string, nodeId: string) => ["spaces", spaceId, "text", nodeId] as const,
  metadata: (spaceId: string, nodeId: string) => ["spaces", spaceId, "metadata", nodeId] as const,
  file: (spaceId: string, nodeId: string) => ["spaces", spaceId, "file", nodeId] as const
};
