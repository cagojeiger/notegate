import type { MarkdownOutlineItem } from "./MarkdownOutlineContext";

export type CollectedMarkdownHeading = MarkdownOutlineItem & {
  element: HTMLHeadingElement;
};

export function collectMarkdownHeadings(root: HTMLElement, idPrefix: string): CollectedMarkdownHeading[] {
  const duplicateCounts = new Map<string, number>();

  return Array.from(root.querySelectorAll<HTMLHeadingElement>(".markdown h1, .markdown h2, .markdown h3, .markdown h4"))
    .flatMap((element, index) => {
      const label = element.textContent?.replace(/\s+/g, " ").trim();
      if (!label) return [];
      const slug = headingSlug(label) || `heading-${index + 1}`;
      const count = (duplicateCounts.get(slug) ?? 0) + 1;
      duplicateCounts.set(slug, count);
      const id = `${idPrefix}-${slug}${count > 1 ? `-${count}` : ""}`;
      element.id = id;
      return [{ id, label, level: Number(element.tagName.slice(1)), element }];
    });
}

export function activeMarkdownHeadingId(root: HTMLElement, headings: CollectedMarkdownHeading[]): string | null {
  if (headings.length === 0) return null;
  const isScrollable = root.scrollHeight > root.clientHeight;
  const isAtBottom = root.scrollHeight - root.clientHeight - root.scrollTop <= 1;
  if (isScrollable && isAtBottom) return headings[headings.length - 1].id;

  const threshold = root.getBoundingClientRect().top + 32;
  let activeId = headings[0].id;

  for (const heading of headings) {
    if (heading.element.getBoundingClientRect().top > threshold) break;
    activeId = heading.id;
  }
  return activeId;
}

function headingSlug(label: string): string {
  return label
    .normalize("NFKC")
    .toLowerCase()
    .replace(/[^\p{Letter}\p{Number}]+/gu, "-")
    .replace(/^-+|-+$/g, "");
}
