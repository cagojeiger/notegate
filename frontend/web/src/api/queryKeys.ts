export const queryKeys = {
  me: ["me"] as const,
  spaces: ["spaces"] as const,
  children: (spaceId: string, nodeId: string) => ["spaces", spaceId, "children", nodeId] as const,
  recent: (spaceId: string) => ["spaces", spaceId, "recent"] as const,
  node: (spaceId: string, nodeId: string) => ["spaces", spaceId, "nodes", nodeId] as const
};
