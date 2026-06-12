import type { Config } from "tailwindcss";

export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        bg: "var(--ng-bg)",
        surface: "var(--ng-surface)",
        panel: "var(--ng-panel)",
        "panel-strong": "var(--ng-panel-strong)",
        border: "var(--ng-border)",
        "border-strong": "var(--ng-border-strong)",
        text: "var(--ng-text)",
        "primary-contrast": "var(--ng-primary-contrast)",
        muted: "var(--ng-muted)",
        faint: "var(--ng-faint)",
        primary: "var(--ng-primary)",
        danger: "var(--ng-danger)",
        success: "var(--ng-success)",
        warning: "var(--ng-warning)"
      },
      fontFamily: {
        ui: "var(--font-ui)",
        mono: "var(--font-mono)"
      }
    }
  },
  plugins: []
} satisfies Config;
