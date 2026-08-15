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

Prefer `shared/ui` instead of repeating:

- cards/panels: use `Card`
- empty containers: use `EmptyState`
- section labels: use `SectionHeader`
- inputs/selects/textareas: use `TextField`, `SelectField`, `TextAreaField`
- tabs: use `Tabs`
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

- Light theme uses low-chroma warm gray chrome around a clean white editor.
- Dark theme uses dim charcoal navigation around a slightly brighter editor.
- Neutral surfaces own hover and selection; muted plum is reserved for focus, links, active borders, and primary actions.
- Normal text targets WCAG 2.2 AA contrast of at least 4.5:1.
- Meaningful controls and focus indicators target at least 3:1.
- Brand assets identify NoteGate; Lucide icons represent actions and objects.
- Theme is local UI state; it does not change backend data.
