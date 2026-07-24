import type { RestNode } from "../../../entities/node/model";
import type { Space } from "../../../entities/space/model";

type MaybePromise<T> = T | Promise<T>;

// Discriminated union of every in-app dialog. Replaces window.prompt/confirm so
// flows match the app tone and stay keyboard/escape friendly.
export type AppDialog =
  | { kind: "prompt"; title: string; label: string; initial: string; placeholder?: string; submitLabel?: string; onSubmit: (value: string) => MaybePromise<void> }
  | { kind: "confirm"; title: string; message: string; danger?: boolean; confirmLabel?: string; onConfirm: () => MaybePromise<void> }
  | { kind: "move"; node: RestNode; space: Space; onMove: (parentId: string) => MaybePromise<void> }
  | { kind: "metadata"; node: RestNode; onSave: (metadata: Record<string, unknown>) => MaybePromise<void> };
