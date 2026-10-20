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
    element.scrollTop = (values?.get(key) as number | undefined) ?? 0;
    const save = () => { values?.set(key, element.scrollTop); };
    element.addEventListener("scroll", save);
    return () => { element.removeEventListener("scroll", save); };
  }, [values, key]);
}
