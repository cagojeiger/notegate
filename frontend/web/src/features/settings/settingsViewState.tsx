import { createContext, useCallback, useContext, useLayoutEffect, useRef, useState, type ReactNode } from "react";

// Session-only UI state. Query results and credentials stay with their existing owners.
const SettingsViewStateContext = createContext<Map<string, unknown> | null>(null);

export function SettingsViewSession({ children }: { children: ReactNode }) {
  const values = useRef(new Map<string, unknown>());
  return <SettingsViewStateContext value={values.current}>{children}</SettingsViewStateContext>;
}

export function useSettingsViewState<T,>(key: string, initial: T | (() => T)) {
  const values = useContext(SettingsViewStateContext);
  const [value, setValue] = useState<T>(() => values?.has(key)
    ? values.get(key) as T
    : typeof initial === "function" ? (initial as () => T)() : initial);
  useLayoutEffect(() => { values?.set(key, value); }, [values, key, value]);
  return [value, setValue] as const;
}

export function useSettingsScroll<T extends HTMLElement = HTMLDivElement>(key: string) {
  const values = useContext(SettingsViewStateContext);
  return useCallback((element: T | null) => {
    if (!element) return;
    const top = (values?.get(key) as number | undefined) ?? 0;
    let restoring = true;
    const observer = new MutationObserver(() => restore());
    const restore = () => {
      element.scrollTop = top;
      restoring = element.scrollTop < top;
      if (!restoring) observer.disconnect();
    };
    restore();
    // A cold query mounts its scroll area before the asynchronous list arrives.
    if (restoring) observer.observe(element, { childList: true, subtree: true });
    const save = () => { if (!restoring) values?.set(key, element.scrollTop); };
    const stopRestoring = () => { restoring = false; observer.disconnect(); save(); };
    element.addEventListener("scroll", save);
    element.addEventListener("wheel", stopRestoring, { passive: true });
    element.addEventListener("pointerdown", stopRestoring);
    element.addEventListener("keydown", stopRestoring);
    return () => {
      observer.disconnect();
      element.removeEventListener("scroll", save);
      element.removeEventListener("wheel", stopRestoring);
      element.removeEventListener("pointerdown", stopRestoring);
      element.removeEventListener("keydown", stopRestoring);
    };
  }, [values, key]);
}
