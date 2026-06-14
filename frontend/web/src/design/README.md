# Frontend design system

Notegate uses a small Lovable-inspired design system: warm neutral surfaces, shallow depth, and explicit shared UI primitives.

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
- `Markdown`

## Theme policy

- Light theme uses warm cream surfaces inspired by Lovable.
- Dark theme keeps the same warm-neutral hue instead of a blue IDE palette.
- Theme is local UI state; it does not change backend data.
