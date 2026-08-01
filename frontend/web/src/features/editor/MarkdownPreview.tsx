import { useId, useLayoutEffect, useRef } from "react";

import type { MarkdownImagePolicy, MarkdownLinkPolicy } from "../../shared/lib/markdownLinks";
import { Markdown } from "./Markdown";
import { useMarkdownOutlineContext, type MarkdownOutlineIdentity } from "./MarkdownOutlineContext";
import { activeMarkdownHeadingId, collectMarkdownHeadings } from "./markdownOutline";

const NAVIGATION_SETTLE_MS = 140;

export function MarkdownPreview({ content, linkPolicy, imagePolicy, outlineIdentity }: { content: string; linkPolicy?: MarkdownLinkPolicy; imagePolicy?: MarkdownImagePolicy; outlineIdentity?: MarkdownOutlineIdentity }) {
  const scrollRootRef = useRef<HTMLDivElement | null>(null);
  const outlineContext = useMarkdownOutlineContext();
  const publishOutline = outlineContext?.publishOutline;
  const clearOutline = outlineContext?.clearOutline;
  const readScrollPosition = outlineContext?.readScrollPosition;
  const writeScrollPosition = outlineContext?.writeScrollPosition;
  const idPrefix = `markdown-outline-${useId().replace(/[^a-zA-Z0-9_-]/g, "")}`;

  useLayoutEffect(() => {
    const root = scrollRootRef.current;
    if (!root || !outlineIdentity || !publishOutline || !clearOutline || !readScrollPosition || !writeScrollPosition) return;

    const headings = collectMarkdownHeadings(root, idPrefix);
    const items = headings.map(({ element: _element, ...item }) => item);
    const savedScrollTop = readScrollPosition(outlineIdentity);
    root.scrollTop = savedScrollTop ?? 0;

    let activeItemId = activeMarkdownHeadingId(root, headings);
    let navigationTargetId: string | null = null;
    let navigationSettleTimer: number | null = null;
    let navigationSettleFrame: number | null = null;
    const publish = () => {
      publishOutline({
        ...outlineIdentity,
        items,
        activeItemId,
        navigate
      });
    };
    const cancelNavigationSettle = () => {
      if (navigationSettleTimer !== null) {
        window.clearTimeout(navigationSettleTimer);
        navigationSettleTimer = null;
      }
      if (navigationSettleFrame !== null) {
        window.cancelAnimationFrame(navigationSettleFrame);
        navigationSettleFrame = null;
      }
    };
    const updateActiveFromScroll = () => {
      const nextActiveItemId = activeMarkdownHeadingId(root, headings);
      if (nextActiveItemId === activeItemId) return;
      activeItemId = nextActiveItemId;
      publish();
    };
    const scheduleNavigationSettle = () => {
      cancelNavigationSettle();
      navigationSettleTimer = window.setTimeout(() => {
        navigationSettleTimer = null;
        navigationSettleFrame = window.requestAnimationFrame(() => {
          navigationSettleFrame = null;
          navigationTargetId = null;
          updateActiveFromScroll();
        });
      }, NAVIGATION_SETTLE_MS);
    };
    const navigate = (itemId: string) => {
      const heading = headings.find((candidate) => candidate.id === itemId)?.element;
      if (!heading) return;
      const top = Math.max(
        0,
        heading.getBoundingClientRect().top - root.getBoundingClientRect().top + root.scrollTop - 16
      );
      navigationTargetId = itemId;
      cancelNavigationSettle();
      if (activeItemId !== itemId) {
        activeItemId = itemId;
        publish();
      }
      if (typeof root.scrollTo === "function") {
        root.scrollTo({
          top,
          behavior: window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth"
        });
      } else {
        root.scrollTop = top;
      }
      writeScrollPosition(outlineIdentity, top);
      scheduleNavigationSettle();
    };
    const handleScroll = () => {
      writeScrollPosition(outlineIdentity, root.scrollTop);
      if (navigationTargetId !== null) {
        scheduleNavigationSettle();
        return;
      }
      updateActiveFromScroll();
    };

    publish();
    root.addEventListener("scroll", handleScroll, { passive: true });
    return () => {
      root.removeEventListener("scroll", handleScroll);
      cancelNavigationSettle();
      writeScrollPosition(outlineIdentity, root.scrollTop);
      clearOutline(outlineIdentity);
    };
  }, [clearOutline, content, idPrefix, outlineIdentity, publishOutline, readScrollPosition, writeScrollPosition]);

  return (
    <div ref={scrollRootRef} className="min-h-0 w-full flex-1 overflow-y-auto px-5 py-8 md:px-6 md:py-10 lg:px-8 lg:py-12" data-testid="markdown-preview-scroll-region">
      <Markdown content={content} linkPolicy={linkPolicy} imagePolicy={imagePolicy} imageViewportRoot={scrollRootRef} />
    </div>
  );
}
