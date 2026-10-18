# Frontend design system

NoteGate uses a compact design system anchored by the brand's ink and paper colors, shallow depth, and explicit shared UI primitives.

## Source of truth

- `theme.css` owns light/dark CSS variables (`--ng-*`).
- `tailwind.config.ts` maps those variables to Tailwind names such as `bg`, `surface`, `panel`, `border`, `text`, and `muted`.
- `tokens.ts` is for TypeScript-level semantic constants only. Do not duplicate raw color values there.

## Component rule

Feature components should own data, state, and events. Shared UI primitives should own repeated visual styling.

Allowed directly in feature/layout files:

- layout utilities: `flex`, `grid`, `gap-*`, `w-*`, `h-*`, `min-h-0`, `overflow-*`
- one-off positioning needed by a layout region

Layout utilities do not override shared chrome height, section padding, or target-size contracts. Page content gutters can differ from Inspector density. Mobile and coarse-pointer controls consume `--ng-min-target-size`; a larger parent row does not enlarge a button hit area.

Prefer `shared/ui` instead of repeating:

- workbench and library headers: use `PanelHeader`; aligned headers share the header-size token
- inspector sections: use `PanelSection`; panel edges own seams and resize handles overlay them
- independent content cards: use `Card`; do not wrap an Inspector in a nested card
- empty containers: use `EmptyState`
- section labels: use `SectionHeader`
- inputs/selects/textareas: use `TextField`, `SelectField`, `TextAreaField`
- tabs: use `Tabs`
- context menus: use `ContextMenu` with `MenuButton` for focus, navigation, placement, and dismissal
- small metadata pills: use `Badge`
- buttons: use `Button` or `IconButton`
- key-value rows: use `MetaRow`

## Current primitive set

- `Button`, `IconButton`, `MenuButton`
- `Modal`
- `Card`, `EmptyState`, `SectionHeader`
- `TextField`, `TextAreaField`, `SelectField`
- `Tabs`
- `Badge`, `MetaRow`

## Theme policy

- Light theme uses lavender-paper chrome around a near-white editor; dark theme uses smoky graphite chrome around a subtly brighter neutral surface.
- Neutral surfaces own hover. Soft violet identifies selected surfaces, while muted violet is reserved for focus, links, active borders, and primary actions.
- Violet should stay below roughly 5% of a typical workbench screen; status colors keep their semantic meaning instead of becoming brand decoration.
- Normal text targets WCAG 2.2 AA contrast of at least 4.5:1.
- Meaningful controls and focus indicators target at least 3:1.
- When a proposed brand swatch misses those thresholds on a real UI surface, the accessible token in `theme.css` takes precedence.
- Brand assets identify NoteGate; Lucide icons represent actions and objects.
- Theme is local UI state; it does not change backend data.

## Layout and state contracts

- Workbench preserves at least 320px for the editor area, clamps displayed sidebar widths without rewriting preferred widths, and overlays the Inspector when docked minimum widths no longer fit.
- Text editor headers collapse secondary controls by pane width and keep those actions in the existing More actions menu, including structured preview/source switching.
- GitLab-inspired theme colors remain separate from the VS Code-inspired workbench composition.
- Space Library creation is the final plus card in the grid, outside Space counts and reordering. Its header reserves room for future view tabs; the rail keeps its quick-create action.
- Reader outer gutters use the same responsive spacing for Markdown and plain text; document typography remains independent from workbench density.
- Data consumers distinguish loading, empty, error, and ready states. Failed reads offer retry instead of presenting missing data as an empty list or a permission value.
- Verify shared changes in light/dark, mobile/touch, narrow desktop, and keyboard flows. Screenshot artifacts show appearance; geometry and behavior assertions verify contracts.
