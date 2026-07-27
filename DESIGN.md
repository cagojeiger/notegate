# Design

## Source of truth

- Status: Active
- Last refreshed: 2026-07-28
- Primary product surfaces: Google SSO login, Space Library, desktop-first workbench, settings, file transfer status, Markdown and structured previews.
- Evidence reviewed: `docs/ui/*`, `frontend/web/src/design/*`, `frontend/web/src/styles/globals.css`, shared UI primitives, auth and layout components, and the 2026-07-23 NoteGate brand asset set.

## Brand

- Personality: Quiet, precise, trustworthy, and tool-like without looking institutional.
- Trust signals: Clear Google-only sign-in, legible states, restrained use of color, and explicit security or recovery copy.
- Avoid: Decorative gradients inside content, security theatre, color-only status, improvised lettermark badges, mixed icon styles, and excessive nested cards.
- Product name: Always write `NoteGate`, including the capital `G`.
- Mark: The open gate and three-node directory tree are the primary symbol. The app icon is used below 32 px; the full symbol or lockup is used at 32 px and above.

## Product goals

- Goals: Make notes and files feel calm to read, make the gate/file-tree model recognizable, and make authentication and system state unambiguous.
- Non-goals: Space Collections, README summaries, or additional authentication providers.
- Success signals: WCAG 2.2 AA contrast, consistent identity across favicon/login/title bar, readable light and dark themes, and no regression in existing UI tests.

## Personas and jobs

- Primary personas: An individual managing private notes, files, and agent access.
- User jobs: Sign in, control which Spaces are available to user MCP, find a Space or node, read and edit content, inspect metadata, and understand sync/upload state.
- Key contexts of use: Long desktop sessions, compact sidebars, Markdown reading, occasional mobile reading and simple actions.

## Information architecture

- Primary navigation: Space Library/Workbench switch, Pinned Space rail, Files/Recent primary sidebar, editor groups, Inspector, Settings. An active Unpinned Space remains in the rail until the user changes destination, without changing its user MCP visibility.
- Core routes/screens: AuthScreen and AppShell with Library and Workbench surfaces.
- Content hierarchy: Product identity and current surface in the title bar; all owned Spaces in one user-ordered Library grid; Space capacity and manual usage checks in the selected Space Inspector; node content in the editor; identity, change protection, settings, metadata, and secondary details in that order in the Node Inspector; app state in the status bar or transient status surfaces. Navigation pinning is separate from User MCP access.

## Design principles

- Reading first: The editor is the cleanest surface and Markdown typography receives more contrast than surrounding chrome.
- Identity is structural: Use the NoteGate mark at product entry points, not as decoration throughout the workbench.
- Progressive disclosure: Keep primary surfaces self-explanatory and move uncommon concepts into contextual help or the relevant Inspector.
- Meaning survives color: Pair status color with text, shape, or icon.
- One visual grammar: Brand assets identify the product; Lucide icons represent actions and objects.
- Tradeoffs: Compact desktop density is retained, but interactive targets remain at least 24 CSS px and visible focus is never removed.

## Visual language

- Color: Brand ink `#17212b` and paper `#f7f9fb` anchor neutral surfaces. Blue is reserved for links, selection, focus, and primary actions. Status colors are semantic and contrast-safe.
- Typography: Apple/system sans for UI and reading; system monospace for code, paths, identifiers, and structured data.
- Spacing/layout rhythm: 4 px base rhythm; 8–12 px control gaps; 16–24 px component spacing; 48 px aligned workbench body headers; generous Markdown reading padding.
- Shape/radius/elevation: 8–10 px controls, 12–16 px panels, no shadow except floating or modal surfaces. Each panel boundary has one 1 px seam; resize handles may use a wider invisible hit target without adding another default line.
- Motion: Short color/opacity transitions plus transform-only card reordering; preserve scroll position and respect reduced motion.
- Imagery/iconography: Official NoteGate SVG/PNG assets for identity. Lucide only for functional icons, normally 16 px with 1.75 px stroke. Auth and onboarding may use a low-contrast Gate Field mark at the screen edge; content surfaces remain flat and undecorated.

## Components

- Existing components to reuse: `Button`, `IconButton`, `Card`, `Field`, `Tabs`, `Modal`, `Markdown`.
- New/changed components: Theme-aware brand mark/lockup, Google sign-in button treatment, branded full-screen status, sortable Space Library cards, Space Inspector controls including usage limits and a secondary usage-check action, and the Node Inspector change-protection state.
- Variants and states: Light/dark identity assets; default/hover/focus/disabled Google button; loading/status auth feedback; selected and dragging Space cards; navigation-pinned/unpinned Spaces; User MCP enabled/disabled Spaces; directly locked, inherited lock, and unlocked nodes.
- Token/component ownership: `theme.css` owns semantic colors. Shared UI owns focus, controls, and repeated visual treatment. Feature components own data and state.

## Accessibility

- Target standard: WCAG 2.2 Level AA.
- Keyboard/focus behavior: 2 px visible outline with offset on links, buttons, fields, summaries, and explicit focus targets. Space reordering uses native keyboard activation on dedicated earlier/later buttons; the drag handle is pointer/touch only.
- Contrast/readability: 4.5:1 for normal text, 3:1 for large text and meaningful UI boundaries; light and dark themes are tested separately.
- Screen-reader semantics: Decorative marks are hidden; identity images have concise names; async feedback uses live status regions; icon-only earlier/later buttons have contextual accessible labels, while pointer-only drag handles are hidden from assistive technology.
- Pointer alternatives: Dragging is never the only way to reorder. Earlier/later buttons provide a single-click and single-tap alternative.
- Reduced motion and sensory considerations: Disable nonessential animation for `prefers-reduced-motion`; never use color as the only status signal.

## Responsive behavior

- Supported breakpoints/devices: Existing desktop/tablet/mobile layout policy remains authoritative.
- Layout adaptations: Login stays centered and bounded; Workbench sidebars and editor behavior remain unchanged; the Space Library uses one column on mobile, two on tablet, three on desktop, and four on wide desktop; the Space Inspector is right-docked on desktop/tablet and inline below the cards on mobile.
- Touch/hover differences: Essential actions do not depend on hover; mobile controls keep touch-safe spacing.

## Interaction states

- Active/current: The current surface, Space, and opened node use a primary edge indicator plus a selection background and semantic current/selected state. Only one surface or Space is current within its navigation scope.
- Loading: Branded but quiet, with visible text and an activity indicator. A pending usage check keeps the current values visible, disables duplicate checks, and reports progress in the Space Inspector.
- Empty: Explain the next available action without decorative illustration.
- Error: Pair semantic color with a clear message and recovery action.
- Success: Pair icon or text with status color.
- Disabled: Lower emphasis while retaining readable labels.
- Offline/slow network: Preserve the existing retryable authentication and upload behavior; do not imply that the session was cleared when it was not.

## Content voice

- Tone: Short, direct, calm.
- Terminology: `NoteGate`, `Google`, `space`, `node`, `Files`, `Recent`, and `Inspector`.
- Microcopy rules: State the user action, not the authentication plumbing. Avoid persistent instructional copy when placement, labels, and contextual help can explain the interaction. The login CTA is `Continue with Google`; AuthGate is not presented as a user-facing provider.

## Implementation constraints

- Framework/styling system: React, TypeScript, Tailwind utilities, and CSS custom properties.
- Design-token constraints: Extend the existing `--ng-*` semantic token layer; do not introduce a second theme system or raw feature-level colors.
- Performance constraints: Serve local optimized SVG/PNG assets; do not add a web-font or icon dependency. The Google CTA follows Google's generated HTML button font stack instead of declaring an unavailable local Google Sans font. PDF preview lazy-loads PDF.js, renders one bounded page at a time, and keeps the current page text layer available.
- Compatibility constraints: Preserve the current OAuth popup and developer API-key fallback behavior.
- Test/screenshot expectations: Typecheck, unit tests, production build, contrast checks, and light/dark login screenshots.

## Open questions

- [ ] Confirm whether a future installed/PWA surface needs platform-specific maskable and monochrome icons. Owner: product. Impact: packaging only.
