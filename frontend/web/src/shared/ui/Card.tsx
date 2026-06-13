import type { HTMLAttributes, ReactNode } from "react";

import { cn } from "../lib/cn";

type CardTone = "default" | "success" | "danger";
type CardPadding = "none" | "sm" | "md";
type CardElement = "div" | "section" | "form" | "ul" | "li";

export function Card({ children, tone = "default", padding = "md", className, as = "div", ...props }: HTMLAttributes<HTMLDivElement | HTMLElement | HTMLFormElement | HTMLUListElement | HTMLLIElement> & { children: ReactNode; tone?: CardTone; padding?: CardPadding; as?: CardElement }) {
  const toneClass = {
    default: "border-border bg-surface",
    success: "border-success/40 bg-success/10",
    danger: "border-danger/40 bg-danger/10"
  }[tone];
  const paddingClass = {
    none: "",
    sm: "p-3",
    md: "p-4"
  }[padding];
  const cardClass = cn("rounded-xl border", toneClass, paddingClass, className);

  if (as === "section") {
    return <section className={cardClass} {...props}>{children}</section>;
  }
  if (as === "form") {
    return <form className={cardClass} {...props}>{children}</form>;
  }
  if (as === "ul") {
    return <ul className={cardClass} {...props}>{children}</ul>;
  }
  if (as === "li") {
    return <li className={cardClass} {...props}>{children}</li>;
  }
  return <div className={cardClass} {...props}>{children}</div>;
}
