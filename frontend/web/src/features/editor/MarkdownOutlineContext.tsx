import { createContext, useCallback, useContext, useMemo, useRef, useState, type ReactNode } from "react";

export type MarkdownOutlineItem = {
  id: string;
  label: string;
  level: number;
};

export type MarkdownOutlineIdentity = {
  groupId: number;
  spaceId: string;
  nodeId: string;
};

export type MarkdownOutlineSnapshot = MarkdownOutlineIdentity & {
  items: MarkdownOutlineItem[];
  activeItemId: string | null;
  navigate: (itemId: string) => void;
};

export type MarkdownInspectorView = "details" | "outline";

type MarkdownOutlineContextValue = {
  outlinesByGroup: Readonly<Record<number, MarkdownOutlineSnapshot>>;
  preferredInspectorView: MarkdownInspectorView;
  setPreferredInspectorView: (view: MarkdownInspectorView) => void;
  publishOutline: (outline: MarkdownOutlineSnapshot) => void;
  clearOutline: (identity: MarkdownOutlineIdentity) => void;
  readScrollPosition: (identity: MarkdownOutlineIdentity) => number | undefined;
  writeScrollPosition: (identity: MarkdownOutlineIdentity, scrollTop: number) => void;
};

const MAX_SAVED_POSITIONS = 200;

const MarkdownOutlineContext = createContext<MarkdownOutlineContextValue | null>(null);

export function MarkdownOutlineProvider({ children }: { children: ReactNode }) {
  const [outlinesByGroup, setOutlinesByGroup] = useState<Record<number, MarkdownOutlineSnapshot>>({});
  const [preferredInspectorView, setPreferredInspectorView] = useState<MarkdownInspectorView>("details");
  const scrollPositions = useRef(new Map<string, number>());

  const publishOutline = useCallback((outline: MarkdownOutlineSnapshot) => {
    setOutlinesByGroup((current) => ({ ...current, [outline.groupId]: outline }));
  }, []);

  const clearOutline = useCallback((identity: MarkdownOutlineIdentity) => {
    setOutlinesByGroup((current) => {
      const currentOutline = current[identity.groupId];
      if (currentOutline?.nodeId !== identity.nodeId || currentOutline.spaceId !== identity.spaceId) return current;
      const next = { ...current };
      delete next[identity.groupId];
      return next;
    });
  }, []);

  const readScrollPosition = useCallback((identity: MarkdownOutlineIdentity) => (
    scrollPositions.current.get(scrollPositionKey(identity))
  ), []);

  const writeScrollPosition = useCallback((identity: MarkdownOutlineIdentity, scrollTop: number) => {
    const key = scrollPositionKey(identity);
    scrollPositions.current.delete(key);
    scrollPositions.current.set(key, scrollTop);
    if (scrollPositions.current.size <= MAX_SAVED_POSITIONS) return;
    const oldestKey = scrollPositions.current.keys().next().value;
    if (oldestKey !== undefined) scrollPositions.current.delete(oldestKey);
  }, []);

  const value = useMemo<MarkdownOutlineContextValue>(() => ({
    outlinesByGroup,
    preferredInspectorView,
    setPreferredInspectorView,
    publishOutline,
    clearOutline,
    readScrollPosition,
    writeScrollPosition
  }), [clearOutline, outlinesByGroup, preferredInspectorView, publishOutline, readScrollPosition, writeScrollPosition]);

  return <MarkdownOutlineContext.Provider value={value}>{children}</MarkdownOutlineContext.Provider>;
}

export function useMarkdownOutlineContext(): MarkdownOutlineContextValue | null {
  return useContext(MarkdownOutlineContext);
}

function scrollPositionKey({ groupId, spaceId, nodeId }: MarkdownOutlineIdentity): string {
  return `${groupId}:${spaceId}:${nodeId}`;
}
