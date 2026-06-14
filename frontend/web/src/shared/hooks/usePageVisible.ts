import { useEffect, useState } from "react";

export function isPageVisible(): boolean {
  return typeof document === "undefined" || document.visibilityState === "visible";
}

export function usePageVisible(): boolean {
  const [visible, setVisible] = useState(isPageVisible);

  useEffect(() => {
    const update = () => setVisible(isPageVisible());
    update();
    document.addEventListener("visibilitychange", update);
    return () => document.removeEventListener("visibilitychange", update);
  }, []);

  return visible;
}
