import { useEffect, useState } from "react";

export function useMediaQuery(query: string): boolean {
  const [matches, setMatches] = useState(() => (typeof window === "undefined" ? false : window.matchMedia(query).matches));
  useEffect(() => {
    const mql = window.matchMedia(query);
    const handler = () => setMatches(mql.matches);
    handler();
    mql.addEventListener("change", handler);
    return () => mql.removeEventListener("change", handler);
  }, [query]);
  return matches;
}

// Mobile = below Tailwind's `md` breakpoint (matches the `max-md:` utility boundary).
export function useIsMobile(): boolean {
  return useMediaQuery("(max-width: 767px)");
}

export function useViewportWidth(): number {
  const [width, setWidth] = useState(() => (typeof window === "undefined" ? 0 : window.innerWidth));
  useEffect(() => {
    let animationFrame: number | null = null;
    const update = () => {
      if (animationFrame !== null) return;
      animationFrame = requestAnimationFrame(() => {
        animationFrame = null;
        setWidth((current) => current === window.innerWidth ? current : window.innerWidth);
      });
    };
    setWidth((current) => current === window.innerWidth ? current : window.innerWidth);
    window.addEventListener("resize", update);
    return () => {
      window.removeEventListener("resize", update);
      if (animationFrame !== null) cancelAnimationFrame(animationFrame);
    };
  }, []);
  return width;
}
