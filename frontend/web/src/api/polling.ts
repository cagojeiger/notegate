export const POLLING = {
  openedNodeMs: 30_000,
  openedNodeJitterMs: 5_000,
  recentMs: 60_000,
  recentJitterMs: 10_000,
  treeChildrenMs: 60_000,
  treeChildrenJitterMs: 10_000
} as const;

export function withPollingJitter(baseMs: number, jitterMs: number): number {
  if (jitterMs <= 0) return baseMs;
  const offset = (Math.random() * 2 - 1) * jitterMs;
  return Math.max(1_000, Math.round(baseMs + offset));
}
