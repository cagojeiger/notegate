# UI layout

The dashboard uses a workbench layout after authentication.

## App root

```text
AppRoot
├─ AuthScreen
└─ AppShell
   ├─ TitleBar
   ├─ Workbench
   │  ├─ ActivityRail
   │  ├─ PrimarySidebar
   │  ├─ EditorArea
   │  └─ AuxiliarySidebar
   └─ StatusBar
```

`AuthScreen` is the login/session recovery screen. It is not an `AppShell` route.

## Desktop map

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│ TitleBar                                                                     │
├──────┬────────────────────┬─────────────────────────────────┬───────────────┤
│      │                    │                                 │               │
│ Acti │ PrimarySidebar     │ EditorArea                      │ Auxiliary     │
│ vity │ Files / Recent     │ EditorGroup[1..3]               │ Sidebar       │
│ Rail │                    │                                 │               │
│      │                    │                                 │               │
├──────┴────────────────────┴─────────────────────────────────┴───────────────┤
│ StatusBar                                                                    │
└──────────────────────────────────────────────────────────────────────────────┘
```

## AuthScreen

Contains:

- product identity
- login action
- login progress/error
- developer API key fallback

Does not contain:

- `TitleBar`
- `Workbench`
- `StatusBar`

## TitleBar

Contains:

- product name and short context
- `PrimarySidebar` toggle
- editor split controls
- theme toggle
- `AuxiliarySidebar` toggle

Rules:

- Center command/search stays empty until the feature is defined.
- Do not show node paths in `TitleBar`.
- Do not duplicate Inspector buttons inside editor groups.
- Layout controls stay on the right across desktop/tablet/mobile.

## ActivityRail

```text
ActivityRail
├─ SpaceRailList
├─ SpaceAddButton
└─ RailFooter
   └─ SettingsButton
```

Rules:

- `SpaceRailList` is scrollable.
- `SpaceAddButton` is visible directly below the space list when the caller can create spaces.
- Settings is fixed at the bottom.
- Space reorder is drag-and-drop on desktop for callers that can manage spaces and persists `sort_order`.
- Account and settings live behind Settings, not in `TitleBar`.

## PrimarySidebar

```text
PrimarySidebar
├─ SidebarHeader
└─ SidebarContent
   ├─ FilesSection
   ├─ SidebarSectionResizeHandle
   └─ RecentSection
```

Rules:

- The sidebar width is user-resizable.
- `FilesSection` and `RecentSection` scroll independently.
- Default height ratio is `FilesSection:RecentSection = 2:1`.
- The divider between sections is the resize handle.
- Section headers have a subtle bottom separator.
- Root `/` is not rendered as a visible row; root children appear at the top level.
- `FilesSection` supports collapse-all.
- `RecentSection` supports view/density controls when implemented.

## EditorArea

```text
EditorArea
└─ EditorGroup[1..3]
   ├─ EditorGroupHeader
   └─ EditorViewport
```

Rules:

- One to three editor groups can be open on desktop.
- New groups open to the right of the active group.
- The add-group action is disabled at three groups.
- Mobile shows one group at a time.
- Empty groups show an empty state with create actions.
- Active group state must be visible even when the group is empty.
- Text opens in preview mode by default.
- Edit mode replaces preview inside the same group.
- Group close is handled in `EditorGroupHeader`.

## AuxiliarySidebar

Contains:

- `InspectorPanel`

Rules:

- Initial view is Inspector.
- The sidebar can be shown even when no node is selected; Inspector then renders an empty state.
- Desktop renders it inline.
- Tablet/mobile render it as overlay/sheet.
- Agent management lives in Settings, not in `AuxiliarySidebar`.

## StatusBar

Contains:

- app readiness/session status
- current space name
- short future runtime indicators

Does not contain:

- current node path
- byte count
- line count
- updated timestamp

Those node details belong in `InspectorPanel`.

## Responsive behavior

| Viewport | Layout |
|---|---|
| Desktop | Full workbench with resizable sidebars and up to three editor groups. |
| Tablet | Same roles; sidebars may become overlays. |
| Mobile | Editor-first. Primary and auxiliary areas open as sheets. One editor group visible. |
