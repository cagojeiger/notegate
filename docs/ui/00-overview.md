# UI overview

`docs/ui` is the dashboard UI contract for Notegate. It defines stable terms, layout ownership, backend data mapping, interaction flows, implementation boundaries, and visual rules.

## Document order

```text
00-overview.md       scope and read order
01-glossary.md       canonical UI terms
02-layout.md         AppShell and Workbench layout
03-information.md    backend data to UI mapping
04-flows.md          user actions and state changes
05-implementation.md frontend code and state ownership
06-visual.md         visual tokens and interaction style
```

## Product shape

Notegate has two top-level surfaces.

```text
AppRoot
├─ AuthScreen      # login/session recovery
└─ AppShell        # authenticated dashboard
```

The dashboard is a workbench UI.

```text
AppShell
├─ TitleBar
├─ Workbench
│  ├─ ActivityRail
│  ├─ PrimarySidebar
│  ├─ EditorArea
│  └─ AuxiliarySidebar
└─ StatusBar
```

## Baseline rules

- `AuthScreen` is separate from `AppShell`.
- `/api/v1/me` is the browser session authority.
- Desktop shows the full workbench.
- Tablet/mobile keep the same layout roles and change presentation to overlays/sheets.
- `EditorArea` is the main reading and editing surface.
- `PrimarySidebar` owns navigation: `FilesSection` and `RecentSection`.
- `AuxiliarySidebar` owns contextual information: Inspector and Agent.
- Long-running history or migration notes do not belong in UI docs.
