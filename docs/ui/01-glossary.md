# UI glossary

This document is the canonical terminology for the dashboard UI.

## Layout terms

| Term | Meaning |
|---|---|
| `AppRoot` | Top-level app entry. Renders `AuthScreen` or `AppShell`. |
| `AuthScreen` | Login/session recovery screen. Does not include the workbench. |
| `AppShell` | Authenticated dashboard frame. Contains `TitleBar`, `Workbench`, and `StatusBar`. |
| `TitleBar` | Top global bar. Holds product context and layout controls. |
| `Workbench` | Main authenticated work area. Contains rail, sidebar, editor, and auxiliary area. |
| `ActivityRail` | Left space rail. Holds spaces, capability-gated add-space action, and settings entry. |
| `PrimarySidebar` | Left navigation sidebar for the active space. Holds `FilesSection` and `RecentSection`. |
| `EditorArea` | Main document/file surface. Holds one to three `EditorGroup`s. |
| `EditorGroup` | Independent editor pane with its own opened node and preview/edit mode. |
| `AuxiliarySidebar` | Right contextual sidebar. Holds the Inspector view. |
| `StatusBar` | Bottom global status strip. Shows short app/session state only. |

## Structural terms

| Term | Meaning |
|---|---|
| `SpaceRailList` | Scrollable list of accessible spaces inside `ActivityRail`. |
| `SpaceAddButton` | Space creation entry directly below `SpaceRailList`; visible only when `/api/v1/me.capabilities.can_create_space` is true. |
| `RailFooter` | Fixed bottom rail area. Holds Settings. |
| `FilesSection` | Hierarchical folder/text/file navigation in `PrimarySidebar`. User-facing label: `Files`. |
| `RecentSection` | Recently updated nodes in `PrimarySidebar`. User-facing label: `Recent`. |
| `SidebarSectionResizeHandle` | Divider between `FilesSection` and `RecentSection`. Controls their height ratio. |
| `EditorGroupHeader` | Header for one editor group. Shows node identity and group actions. |
| `EditorViewport` | Body area inside an editor group. Renders folder, text, file, or empty state. |

## View terms

| Term | Meaning |
|---|---|
| `InspectorPanel` | Node metadata and properties. |
| `AgentPanel` | Reserved future contextual agent surface; current agent management lives in Settings. |
| `TextPreview` | Read-only text preview surface. |
| `TextEditor` | Text edit surface. |
| `MarkdownPreview` | Markdown renderer. |
| `StructuredPreview` | JSON/JSONL/YAML/TOML tree/source renderer. |
| `FileDetailView` | File metadata and download surface. |
| `FolderDetailView` | Folder detail surface. |

## Naming rules

- Layout names describe where an area lives.
- View names describe what content they render.
- Use `FilesSection` for the sidebar file hierarchy. Avoid user-facing `Tree` except for structured data mode labels such as `Tree`/`Source`.
- Use `AuxiliarySidebar` for the right area. `InspectorPanel` is only one view inside it.
- Use `EditorGroup` for split panes. Do not call groups tabs or panels.

## Avoided names

```text
LeftPanel
RightPanel
MenuPanel
SideMenu
FilePanel
MainPanel
Content
Body
Panel      # standalone name; use InspectorPanel/AgentPanel/etc.
TreeSection
BottomPanel
EditorInfoBar
```
