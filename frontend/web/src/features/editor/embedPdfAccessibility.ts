export function observeEmbedPdfAccessibility(root: ShadowRoot): () => void {
  const applySemantics = () => {
    root.querySelectorAll("svg").forEach((icon) => {
      icon.removeAttribute("role");
      icon.setAttribute("aria-hidden", "true");
    });
    root.querySelectorAll("img").forEach((image) => {
      image.setAttribute("alt", "");
      image.setAttribute("role", "presentation");
    });
    root.querySelectorAll<HTMLInputElement>('input[inputmode="numeric"]').forEach((input) => {
      input.setAttribute("aria-label", "Current page");
    });
    root.querySelectorAll<HTMLElement>('[style*="overflow: auto"]').forEach((scroller) => {
      scroller.setAttribute("tabindex", "0");
      scroller.setAttribute("role", "region");
      scroller.setAttribute("aria-label", "PDF pages");
    });
    root.querySelectorAll<HTMLElement>('[role="tablist"]').forEach((tabs) => {
      tabs.setAttribute("role", "toolbar");
      tabs.setAttribute("aria-label", "PDF viewing mode");
    });
    root.querySelectorAll<HTMLButtonElement>("button").forEach((button) => {
      if (!button.getAttribute("aria-label") && !button.textContent?.trim()) {
        button.setAttribute("aria-label", "More PDF options");
      }
    });
  };

  applySemantics();
  const observer = new MutationObserver(applySemantics);
  observer.observe(root, { childList: true, subtree: true });
  return () => observer.disconnect();
}
