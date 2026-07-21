import { useCallback, useEffect, useRef } from "react";

export function usePointerDrag() {
  const cleanupRef = useRef<(() => void) | null>(null);

  const stop = useCallback(() => {
    cleanupRef.current?.();
  }, []);

  useEffect(() => stop, [stop]);

  return useCallback((onMove: (event: PointerEvent) => void) => {
    stop();

    const move = (event: PointerEvent) => onMove(event);
    const cleanup = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", cleanup);
      window.removeEventListener("pointercancel", cleanup);
      window.removeEventListener("blur", cleanup);
      document.body.classList.remove("select-none");
      if (cleanupRef.current === cleanup) cleanupRef.current = null;
    };

    cleanupRef.current = cleanup;
    document.body.classList.add("select-none");
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", cleanup);
    window.addEventListener("pointercancel", cleanup);
    window.addEventListener("blur", cleanup);
  }, [stop]);
}
