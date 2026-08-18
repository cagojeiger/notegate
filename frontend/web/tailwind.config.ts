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
        seam: "var(--ng-seam)",
        text: "var(--ng-text)",
        "primary-contrast": "var(--ng-primary-contrast)",
        muted: "var(--ng-muted)",
        faint: "var(--ng-faint)",
        primary: "var(--ng-primary)",
        link: "var(--ng-link)",
        danger: "var(--ng-danger)",
        success: "var(--ng-success)",
        warning: "var(--ng-warning)",
        info: "var(--ng-info)"
      },
      fontFamily: {
        ui: "var(--font-ui)",
        reading: "var(--font-reading)",
        mono: "var(--font-mono)"
      },
      fontSize: {
        workbench: ["var(--ng-workbench-font-size)", { lineHeight: "var(--ng-workbench-line-height)" }]
      },
      spacing: {
        "workbench-header": "var(--ng-workbench-header-size)",
        "workbench-control": "var(--ng-workbench-control-size)",
        "workbench-row": "var(--ng-workbench-row-size)",
        "tree-row": "var(--ng-tree-row-size)",
        "workbench-status": "var(--ng-workbench-status-size)"
      },
      borderRadius: {
        workbench: "var(--ng-workbench-radius)",
        "workbench-surface": "var(--ng-workbench-surface-radius)"
      }
    }
  },
  plugins: []
} satisfies Config;
