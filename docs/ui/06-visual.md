# UI visual style

Notegate uses a calm personal workbench style: warm in light mode, graphite in dark mode, low visual noise, and clear reading surfaces.

## Direction

- The app should feel like a focused personal workspace, not an admin console.
- Light mode uses warm neutral surfaces.
- Dark mode uses neutral graphite, not brown/sepia.
- Primary color is reserved for primary actions and selected states.
- Toolbar chrome stays quiet until hover/focus.
- Text and file reading surfaces should not be wrapped in unnecessary nested cards.

## Theme tokens

### Light

```text
background       #f7f4ed
surface          #fcfbf8
editor           #fcfbf8
panel            #f0ede5
panel-strong     #e8e2d7
border           rgba(28,28,28,0.16)
border-strong    rgba(28,28,28,0.30)
seam             rgba(28,28,28,0.10)
selection        rgba(28,28,28,0.08)
hover            rgba(28,28,28,0.05)
text             #1c1c1c
muted            rgba(28,28,28,0.62)
faint            rgba(28,28,28,0.36)
primary          #1c1c1c
primary-hover    #34302a
primary-contrast #fcfbf8
danger           #c2410c
success          #15803d
warning          #b45309
```

### Dark

```text
background       #1f1f1f
surface          #252525
editor           #1f1f1f
panel            #2b2b2b
panel-strong     #363636
border           rgba(255,255,255,0.14)
border-strong    rgba(255,255,255,0.26)
seam             rgba(255,255,255,0.09)
selection        rgba(255,255,255,0.10)
hover            rgba(255,255,255,0.07)
text             #f4f1ea
muted            rgba(244,241,234,0.64)
faint            rgba(244,241,234,0.38)
primary          #f4f1ea
primary-hover    #ffffff
primary-contrast #1c1c1c
danger           #fb7185
success          #4ade80
warning          #facc15
```

## Typography

- Font stack: `-apple-system`, `BlinkMacSystemFont`, `SF Pro Text`, `Inter`, `system-ui`.
- UI chrome uses 12-14px.
- Reading text uses 16px or larger with line-height at least 1.55.
- Headings use weight 600.
- Avoid heavy 700+ display weight.

## Shape and depth

- Button/input radius: 8-10px.
- Panel/card radius: 12-16px.
- Space avatar radius: 10-12px.
- Use separators and surface tone before shadows.
- Use shadows only for popovers/dialogs/focus where separation is needed.

## Region style rules

### TitleBar

- Keep center area empty until command/search is defined.
- Right side holds layout/theme controls.
- Buttons are quiet by default and obvious on hover/focus.

### ActivityRail

- Selected space is clear without heavy borders on every item.
- Add-space action stays directly below the dynamic space list.
- Settings stays at the bottom.

### PrimarySidebar

- Use source-list density.
- Rows use subtle hover and selected fills.
- Do not draw borders around every row.
- Files and Recent headers use a subtle bottom separator.

### EditorArea

- Document body is the cleanest surface in the app.
- Plain text appears like a simple note, not a nested code card.
- Markdown uses readable prose spacing.
- Code fences use syntax highlighting.
- Mermaid fences render diagrams.
- JSON/JSONL/YAML/TOML use structured `Tree` and raw `Source` modes.
- Structured expand/collapse controls live near the document name in the header.
- Edit mode shows line numbers.

### AuxiliarySidebar

- Inspector remains visible with an empty state when no node is selected.
- Inspector uses grouped information with low visual competition.
- Metadata warning is present but not visually dominant.

## Interaction states

- Hover changes background or opacity enough to signal clickability.
- Keyboard focus is visible.
- Disabled controls keep shape and reduce emphasis.
- Success uses short toast or status text.
- Destructive actions require confirmation.

## Responsive style

- Desktop prioritizes full workbench visibility.
- Tablet/mobile prioritize the editor and show sidebars as overlays/sheets.
- Touch targets should be at least 44px where possible.
