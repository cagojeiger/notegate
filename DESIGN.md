# Design

## Source of truth

- Status: Active
- Last refreshed: 2026-07-23
- Primary product surfaces: Google SSO login, desktop-first workbench, settings, file transfer status, Markdown and structured previews.
- Evidence reviewed: `docs/ui/*`, `frontend/web/src/design/*`, `frontend/web/src/styles/globals.css`, shared UI primitives, auth and layout components, and the 2026-07-23 NoteGate brand asset set.

## Brand

- Personality: Quiet, precise, trustworthy, and tool-like without looking institutional.
- Trust signals: Clear Google-only sign-in, legible states, restrained use of color, and explicit security or recovery copy.
- Avoid: Decorative gradients inside content, security theatre, color-only status, improvised lettermark badges, mixed icon styles, and excessive nested cards.
- Product name: Always write `NoteGate`, including the capital `G`.
- Mark: The open gate and three-node directory tree are the primary symbol. The app icon is used below 32 px; the full symbol or lockup is used at 32 px and above.

## Product goals

- Goals: Make notes and files feel calm to read, make the gate/file-tree model recognizable, and make authentication and system state unambiguous.
- Non-goals: Reworking information architecture, changing backend behavior, or adding authentication providers.
- Success signals: WCAG 2.2 AA contrast, consistent identity across favicon/login/title bar, readable light and dark themes, and no regression in existing UI tests.

## Personas and jobs

- Primary personas: An individual managing private notes, files, and agent access.
- User jobs: Sign in, find a space or node, read and edit content, inspect metadata, and understand sync/upload state.
- Key contexts of use: Long desktop sessions, compact sidebars, Markdown reading, occasional mobile reading and simple actions.

## Information architecture

- Primary navigation: Space rail, Files/Recent primary sidebar, editor groups, Inspector, Settings.
- Core routes/screens: AuthScreen and AppShell.
- Content hierarchy: Product identity and current space in the title bar; node content in the editor; details in Inspector; app state in the status bar or transient status surfaces.

## Design principles

- Reading first: The editor is the cleanest surface and Markdown typography receives more contrast than surrounding chrome.
- Identity is structural: Use the NoteGate mark at product entry points, not as decoration throughout the workbench.
- Meaning survives color: Pair status color with text, shape, or icon.
- One visual grammar: Brand assets identify the product; Lucide icons represent actions and objects.
- Tradeoffs: Compact desktop density is retained, but interactive targets remain at least 24 CSS px and visible focus is never removed.

## Visual language

- Color: Brand ink `#17212b` and paper `#f7f9fb` anchor neutral surfaces. Blue is reserved for links, selection, focus, and primary actions. Status colors are semantic and contrast-safe.
- Typography: Apple/system sans for UI and reading; system monospace for code, paths, identifiers, and structured data.
- Spacing/layout rhythm: 4 px base rhythm; 8–12 px control gaps; 16–24 px component spacing; generous Markdown reading padding.
- Shape/radius/elevation: 8–10 px controls, 12–16 px panels, no shadow except floating or modal surfaces.
- Motion: Short color/opacity transitions only; respect reduced motion.
- Imagery/iconography: Official NoteGate SVG/PNG assets for identity. Lucide only for functional icons, normally 16 px with 1.75 px stroke. Auth and onboarding may use a low-contrast Gate Field mark at the screen edge; content surfaces remain flat and undecorated.

## Components

- Existing components to reuse: `Button`, `IconButton`, `Card`, `Field`, `Tabs`, `Modal`, `Markdown`.
- New/changed components: Theme-aware brand mark/lockup, Google sign-in button treatment, branded full-screen status.
- Variants and states: Light/dark identity assets; default/hover/focus/disabled Google button; loading/status auth feedback.
- Token/component ownership: `theme.css` owns semantic colors. Shared UI owns focus, controls, and repeated visual treatment. Feature components own data and state.

## Accessibility

- Target standard: WCAG 2.2 Level AA.
- Keyboard/focus behavior: 2 px visible outline with offset on links, buttons, fields, summaries, and explicit focus targets.
- Contrast/readability: 4.5:1 for normal text, 3:1 for large text and meaningful UI boundaries; light and dark themes are tested separately.
- Screen-reader semantics: Decorative marks are hidden; identity images have concise names; async auth feedback uses a live status region; icon-only buttons have accessible labels.
- Reduced motion and sensory considerations: Disable nonessential animation for `prefers-reduced-motion`; never use color as the only status signal.

## Responsive behavior

- Supported breakpoints/devices: Existing desktop/tablet/mobile layout policy remains authoritative.
- Layout adaptations: Login stays centered and bounded; workbench sidebars and editor behavior remain unchanged.
- Touch/hover differences: Essential actions do not depend on hover; mobile controls keep touch-safe spacing.

## Interaction states

- Loading: Branded but quiet, with visible text and an activity indicator.
- Empty: Explain the next available action without decorative illustration.
- Error: Pair semantic color with a clear message and recovery action.
- Success: Pair icon or text with status color.
- Disabled: Lower emphasis while retaining readable labels.
- Offline/slow network: Preserve the existing retryable authentication and upload behavior; do not imply that the session was cleared when it was not.

## Content voice

- Tone: Short, direct, calm.
- Terminology: `NoteGate`, `Google`, `space`, `node`, `Files`, `Recent`, and `Inspector`.
- Microcopy rules: State the user action, not the authentication plumbing. The login CTA is `Continue with Google`; AuthGate is not presented as a user-facing provider.

## Implementation constraints

- Framework/styling system: React, TypeScript, Tailwind utilities, and CSS custom properties.
- Design-token constraints: Extend the existing `--ng-*` semantic token layer; do not introduce a second theme system or raw feature-level colors.
- Performance constraints: Serve local optimized SVG/PNG assets; do not add a web-font or icon dependency.
- Compatibility constraints: Preserve the current OAuth popup and developer API-key fallback behavior.
- Test/screenshot expectations: Typecheck, unit tests, production build, contrast checks, and light/dark login screenshots.

## Open questions

- [ ] Confirm whether a future installed/PWA surface needs platform-specific maskable and monochrome icons. Owner: product. Impact: packaging only.
