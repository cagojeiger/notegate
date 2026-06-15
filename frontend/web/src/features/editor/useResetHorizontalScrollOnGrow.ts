import { useEffect, type RefObject } from "react";

export function useResetHorizontalScrollOnGrow<T extends HTMLElement>(ref: RefObject<T | null>) {
  useEffect(() => {
    const element = ref.current;
    if (!element || typeof ResizeObserver === "undefined") return;

    return observeHorizontalScrollOnGrow(element);
  }, [ref]);
}

export function useResetHorizontalScrollDescendantsOnGrow<T extends HTMLElement>(ref: RefObject<T | null>, selector: string) {
  useEffect(() => {
    const element = ref.current;
    if (!element || typeof ResizeObserver === "undefined") return;

    const disconnectors = Array.from(element.querySelectorAll<HTMLElement>(selector)).map(observeHorizontalScrollOnGrow);
    return () => {
      for (const disconnect of disconnectors) disconnect();
    };
  }, [ref, selector]);
}

function observeHorizontalScrollOnGrow(element: HTMLElement) {
  let previousWidth = element.clientWidth;
  const observer = new ResizeObserver(() => {
    const currentWidth = element.clientWidth;
    if (currentWidth > previousWidth && element.scrollLeft > 0) {
      element.scrollLeft = 0;
    }
    previousWidth = currentWidth;
  });
  observer.observe(element);
  return () => observer.disconnect();
}
