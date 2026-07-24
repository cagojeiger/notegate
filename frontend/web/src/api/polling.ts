export const POLLING = {
  usagePendingMs: 3_000,
  spaceChangesMs: 30_000,
  spaceChangesJitterMs: 5_000
} as const;

export function withPollingJitter(baseMs: number, jitterMs: number): number {
  if (jitterMs <= 0) return baseMs;
  const offset = (Math.random() * 2 - 1) * jitterMs;
  return Math.max(1_000, Math.round(baseMs + offset));
}
