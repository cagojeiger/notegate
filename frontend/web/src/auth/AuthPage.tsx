import { useId, type ReactNode } from "react";

import { BrandLockup } from "../shared/ui";
import { persistTheme, useUiStore } from "../stores/uiStore";

export function AuthPage({ title, children, footer, wide = false }: {
  title: string;
  children: ReactNode;
  footer?: ReactNode;
  wide?: boolean;
}) {
  const titleId = useId();
  const theme = useUiStore((state) => state.theme);
  const toggleTheme = useUiStore((state) => state.toggleTheme);
  const nextTheme = theme === "light" ? "dark" : "light";

  return (
    <main className="ng-auth-screen">
      <div className={`auth-shell${wide ? "" : " auth-shell--compact"}`}>
        <header className="auth-header">
          <a className="auth-brand" href="/" aria-label="NoteGate home"><BrandLockup className="w-[190px]" /></a>
          <button type="button" className="auth-theme" aria-label={`Use ${nextTheme} theme`} onClick={() => {
            toggleTheme();
            persistTheme(nextTheme);
          }}>{nextTheme === "dark" ? "Dark" : "Light"}</button>
        </header>
        <section className="auth-card" aria-labelledby={titleId}>
          <h1 id={titleId}>{title}</h1>
          {children}
        </section>
        <footer className="auth-footer">
          {footer}
          <nav className="auth-links" aria-label="Legal and support">
            <a href="/terms">Terms</a>
            <a href="/privacy">Privacy</a>
            <a href="/support">Support</a>
          </nav>
        </footer>
      </div>
    </main>
  );
}
