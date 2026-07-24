const SEMANTIC_TARGETS = [
  "svg",
  "img",
  'input[inputmode="numeric"]',
  '[style*="overflow: auto"]',
  '[role="tablist"]',
  "button"
].join(",");

function applySemantics(scope: ShadowRoot | Element) {
  const targets = Array.from(scope.querySelectorAll(SEMANTIC_TARGETS));
  if (scope instanceof Element && scope.matches(SEMANTIC_TARGETS)) targets.unshift(scope);

  targets.forEach((target) => {
    if (target.matches("svg")) {
      const icon = target as SVGElement;
      icon.removeAttribute("role");
      icon.setAttribute("aria-hidden", "true");
    } else if (target.matches("img")) {
      const image = target as HTMLImageElement;
      image.setAttribute("alt", "");
      image.setAttribute("role", "presentation");
    } else if (target.matches('input[inputmode="numeric"]')) {
      const input = target as HTMLInputElement;
      input.setAttribute("aria-label", "Current page");
    } else if (target.matches('[style*="overflow: auto"]')) {
      const scroller = target as HTMLElement;
      scroller.setAttribute("tabindex", "0");
      scroller.setAttribute("role", "region");
      scroller.setAttribute("aria-label", "PDF pages");
    } else if (target.matches('[role="tablist"]')) {
      const tabs = target as HTMLElement;
      tabs.setAttribute("role", "toolbar");
      tabs.setAttribute("aria-label", "PDF viewing mode");
    } else if (target.matches("button")) {
      const button = target as HTMLButtonElement;
      if (!button.getAttribute("aria-label") && !button.textContent?.trim()) {
        button.setAttribute("aria-label", "More PDF options");
      }
    }
  });
}

export function observeEmbedPdfAccessibility(root: ShadowRoot): () => void {
  const pendingRoots = new Set<Element>();
  let animationFrame: number | null = null;
  const flush = () => {
    animationFrame = null;
    const roots = Array.from(pendingRoots);
    pendingRoots.clear();
    roots
      .filter((candidate) => !roots.some((other) => other !== candidate && other.contains(candidate)))
      .forEach(applySemantics);
  };

  applySemantics(root);
  const observer = new MutationObserver((mutations) => {
    mutations.forEach((mutation) => {
      if (mutation.target instanceof Element) pendingRoots.add(mutation.target);
      mutation.addedNodes.forEach((node) => {
        if (node instanceof Element) pendingRoots.add(node);
      });
    });
    if (pendingRoots.size > 0 && animationFrame === null) {
      animationFrame = requestAnimationFrame(flush);
    }
  });
  observer.observe(root, { childList: true, subtree: true });
  return () => {
    observer.disconnect();
    pendingRoots.clear();
    if (animationFrame !== null) cancelAnimationFrame(animationFrame);
  };
}
