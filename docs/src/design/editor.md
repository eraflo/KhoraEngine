# Khora Editor — Design Document

**An editor that thinks.**

A hi-fidelity design system for the Khora Engine editor — a 2D/3D Rust engine built on a self-optimizing Symbiotic Adaptive Architecture. This document captures the visual language, architectural choices, and panel-by-panel rationale behind the design.

- Document — Khora Editor Design v1.0
- Status — Hi-fi mockup
- Date — May 2026

---

## Contents

1. Design principles
2. Brand & identity
3. Color system
4. Typography
5. Layout & spine
6. Panels
7. Viewport
8. Command palette
9. Control Plane
10. Interactions
11. Decisions log
12. Open questions

---

## 01 — Design principles

The five rules everything else descends from.

### 1. The work is the hero
Chrome retreats. Toolbars are thin, panels are quiet, the viewport breathes. The user's scene, code, or canvas is always the loudest thing on screen.

### 2. The engine has a mind — show it
Khora is built on a self-optimizing architecture (GORNA). The editor must surface that intelligence — telemetry as ambient signal, not a separate dashboard. The user should *feel* the engine thinking.

### 3. Density without density anxiety
Engine editors fail by hiding everything in nested menus or by drowning the user in chips. We use a **Spine** (mode rail) and **Pills** (compact groups) so dense information stays scannable.

### 4. Mode-first, not panel-first
Unity and Unreal let you arrange panels freely — and most users keep the default forever. We commit to opinionated layouts per Mode (Scene, Canvas, Graph, Animation, Shader, Control Plane). Power users can still customize, but the default is excellent.

### 5. Calm color, loud signal
The base palette is near-monochrome (deep blue-black + warm silver). Color is reserved for **state** — gold for the active selection, green for healthy telemetry, amber for warnings. When something is colorful, it matters.

---

## 02 — Brand & identity

### The mark
A faceted diamond of four kite shapes — the four cardinal architectural pillars of Khora (Renderer · Agents · Assets · Editor) folded into a single shape. Read as a gem (precision, value), a compass rose (orientation), or a pulse diamond (the engine's heartbeat).

### Where the mark lives
- **Title bar** — small, in the brand pill, before `KhoraEngine`
- **Spine top** — slightly larger with a subtle glow, anchoring the mode rail
- **Empty viewport** — giant watermark at 5% opacity behind the grid
- **Command palette** — replaces the search-icon glyph
- **Status bar** — small leading glyph, slow pulse on async work

### Mood
Not industrial. Not playful. *Instrumental* — like a workshop where every tool has earned its hook on the wall.

| | | | |
|---|---|---|---|
| **deep** | **precise** | **awake** | **honest** |
| Surface | Type | Motion | Voice |

---

## 03 — Color system

The palette is designed in **OKLCH** for perceptual uniformity. Hues hold their identity across light/dark, and our gold stays gold across alpha tints.

### Foundation — the deep field
The shell sits in a narrow band of blue-black, never pure black. Pure black creates harsh contrast that fights with code syntax colors and 3D viewport content.

| Role | OKLCH | Use |
|---|---|---|
| **bg-0** Void | `oklch(0.14 0.020 265)` | App background — rarely seen directly |
| **bg-1** Shell | `oklch(0.16 0.022 265)` | Panel surfaces, title bar |
| **bg-2** Raised | `oklch(0.18 0.022 265)` | Cards, hover states |
| **bg-3** Elevated | `oklch(0.21 0.024 265)` | Modals, command palette |

### Foreground — warm silver
Text and icons sit in a warm, slightly desaturated silver. Cooler grays read as clinical; warmer grays as cheap. We split the difference and bias warm.

### Accents — three signals only
- **Gold** `oklch(0.78 0.13 75)` — selection, active mode, focus, brand
- **Green** `oklch(0.65 0.18 145)` — healthy telemetry, success
- **Amber** `oklch(0.75 0.16 65)` — warning, GORNA suggestion ready

> **Color is a load-bearing element.** Gold means *now*. Silver means *noticed*. Everything else is structure.

---

## 04 — Typography

Three families, each with one job. No more.

### The trio
- **Geist** — Sans for UI, labels, body. Designed for software interfaces; neutral but not generic.
- **Geist Mono** — Code, telemetry, paths, numbers. Pairs perfectly with Geist Sans.
- **Fraunces** — Display only. Editorial serif with optical sizing — used for hero titles and section emphasis. Adds gravity without ceremony.

### The scale
| Role | Sample | Spec |
|---|---|---|
| Hero | *An editor that thinks.* | Fraunces 56, opsz 144, italic |
| H2 | Color system | Fraunces 32, regular |
| H3 | Section title | Geist 18, weight 600 |
| Body | Default reading text | Geist 14, line-height 1.6 |
| Label | EXECUTION TIMING | Geist Mono 11, uppercase, letter-spacing 0.08em |
| Engine | `khora-agents · 7.04 / 16.67ms` | Geist Mono 12 |

### Voice & microcopy
The editor talks back. How it talks matters. We aim for **plainspoken with technical precision** — never marketing, never apologetic, never cute.

✓ **Yes**
> Build failed — 3 errors in `renderer.rs`
>
> 17 entities skipped. View log →
>
> GORNA recommends MeshletPipeline. Apply?

✗ **No**
> Oops! Something went wrong 😔
>
> Some entities couldn't be processed.
>
> ✨ Smart suggestion: try MeshletPipeline!

### Rules of voice
- **Numbers before adjectives.** *"7.04 ms"*, not *"fast"*.
- **The engine has a name.** When GORNA suggests something, attribute it. The user is collaborating with a system, not receiving advice from a mascot.
- **No exclamation marks.** The work is serious. Praise feels condescending; warnings shout louder when written calmly.
- **Sentence case for everything.** Title Case is for documents and brand names; UI strings are sentences.
- **Verbs in the user's voice.** Buttons say `Apply`, not `Applying`. The user is the agent.
- **No emoji in product UI.** The diamond is the only mark.

---

## 05 — Layout & spine

Most engine editors waste 220 px on a left sidebar that simply switches workspaces. Khora replaces that with a 48 px **Spine** — a vertical mode rail welded to the left edge — and gives the saved real estate back to the work.

### Anatomy
- **Title bar** — 44 px. Brand pill (logo + project), centered window controls, account.
- **Spine** — 48 px wide, full height. Six modes as icon buttons; active mode lit gold.
- **Workbench** — everything to the right of the Spine. Layout determined by current Mode.
- **Status bar** — 28 px. Engine state, FPS, build status, GORNA pulse.

### Modes
1. **Scene** — 3D editing (default)
2. **Canvas** — 2D layout & UI
3. **Graph** — Node-based logic / shader
4. **Animation** — Timeline & curves
5. **Shader** — Code editor with live preview
6. **Control Plane** — Engine telemetry & GORNA stream

> **A panel that demands attention every second isn't a panel — it's an alarm.** We design for ambient awareness, not constant interruption.

---

## 06 — Panels

Each Mode has an opinionated panel layout. Below: Scene mode.

### Hierarchy (left, 280 px)
Tree of entities. Indented with thin guide lines — never with chevron-only disclosure. Icons match entity type (mesh, light, camera, group). Selection = gold left-edge bar + raised background.

### Inspector (right, 320 px)
Stacked component cards. Each card is collapsible, with a 6 px gold accent on hover. Numeric fields are draggable scrubbers (no spinners) — drag horizontally to change value, modifier keys for precision.

### Viewport (center, fluid)
The 3D scene. Floating gizmo overlay top-left (move/rotate/scale). Coordinate readout bottom-right in mono. View modes (wireframe, lit, normals) as a floating segmented pill, top-right.

### Bottom dock (180 px, retractable)
Tabbed: **Assets** (grid of project files) · **Console** (filtered log) · **GORNA** (engine suggestions). Drag the divider up to expand; collapses to 28 px tab strip.

---

## 07 — Viewport

The viewport is the most important rectangle in the editor. Everything else exists to serve it.

### Empty state
A subtle 5% diamond watermark, centered. Above it, a single line of microcopy: *"Drag assets here, or press ⌘N for a new entity."* Nothing else.

### Active state
- **Grid** — 1m primary lines at 10% opacity; 0.1m secondary at 4%. Origin marked with thin gold cross.
- **Gizmo** — Three-axis (X red, Y green, Z blue) with a unified center sphere for screen-space drag. Always above the grid, below selection outlines.
- **Selection outline** — 2 px gold inner stroke, 1 px void outer stroke (so it reads on any background).
- **Coordinate HUD** — Bottom-right, mono, 4 decimals: `x: 12.3450  y: 0.0000  z: -4.2010`

### Floating controls
View pill (top-right) — segmented: `Lit · Wire · Normals · UV`. Camera pill (top-left) — `Persp · Top · Front · Side`. Both use the standard Pill component (12 px height, 999 radius).

### Selection scrubber
Drag any numeric field horizontally to scrub. No arrows. No spinners. The cursor becomes a gold double-arrow during drag; ticks snap to integer units (toggle off with Alt).

---

## 08 — Command palette

Press `⌘K`. The editor's most important keyboard shortcut.

### Layout
A centered modal, ~640 px wide, 60% viewport height max. Top: search field with diamond glyph leading. Below: results in a flat ranked list, OR a **quadrant view** if the query is empty.

### Quadrant view (empty query)
Four boxes, equal weight: **Recent** · **Suggested** (from GORNA) · **Modes** · **Tools**. Each shows three to five items. Hover lights the box gold; arrow keys move between quadrants then between items.

### Result item
- Diamond glyph (action type) — 12 px, fg-3
- Label — Geist 14
- Path or category — Geist Mono 11, fg-3
- Shortcut — right-aligned, mono 11, kbd-styled

The currently-selected result has a gold left bar (3 px) and bg-3 background. Enter activates; Esc closes.

---

## 09 — Control Plane

The DCC workspace. Where the engine's mind becomes visible.

### Why it exists
Most engines hide their internals behind a profiler you launch separately. Khora's whole pitch is the self-optimizing architecture — so the engine's internal state should be a **first-class workspace**, not a popup.

### Anatomy
- **Lane Timeline** (top, 60% height) — Horizontal lanes per subsystem (renderer, agents, physics, audio, IO). Each lane shows execution windows as colored bands. Hover any band for a tooltip with timing breakdown.
- **GORNA Stream** (bottom-left) — A live feed of the optimizer's reasoning. Each entry: timestamp · subsystem · suggestion · accept/reject affordance.
- **Meters Wall** (bottom-right) — Grid of small gauges: frame time, GPU %, memory, agent budget, assets pending. All meters share the same visual grammar (silver track, gold fill, mono readout below).

### The Lane Timeline is the answer to *"what is the engine doing right now?"* — and the GORNA Stream is the answer to *"why?"*

### Meter anatomy
- Background — bg-2 with 1 px line-soft border
- Track — silver at 12% opacity
- Fill — gold (or green if "good", amber if "warn")
- Readout — mono 14 below, with unit in fg-3 to its right
- Hover — opacity 1, gold ring
- Drag — fill gold, snapping ticks every 1 unit (toggleable)

---

## 10 — Interactions

How the editor moves.

### Motion principles
- **Fast first frame.** Anything that responds to user input must move within 16 ms. Tweens that delay feedback are forbidden.
- **150 ms is the default.** Most state changes (panel collapse, tab switch, hover) finish in 150 ms with `cubic-bezier(0.2, 0, 0, 1)`.
- **300 ms for spatial.** Modes that change layout (entering Control Plane, opening the palette) take 300 ms — long enough to read the spatial change, short enough to not feel slow.
- **No bounces. No springs. Ever.** This isn't a consumer app.

### Drag
- Numeric fields scrub on horizontal drag — gold cursor, mono readout follows the cursor.
- Panel dividers resize on drag — 4 px hit area, gold highlight on hover.
- Hierarchy entries reorder on vertical drag — ghosted item follows cursor, drop zones show as 2 px gold lines between siblings.

### Hover
Hover is information, not decoration. Hover any meter for breakdown; hover any timeline band for a tooltip; hover any pill for its full label. Hover delay: 200 ms (long enough to ignore mouse-overs, short enough to feel instant when intended).

### Keyboard
Every action has a keyboard path. The command palette is the master key. Mode switching is `⌘1` through `⌘6`. Selection moves with arrow keys in any tree or list. Esc always retreats one level (close modal → deselect → exit mode).

---

## 11 — Decisions log

Choices we made, and what we said no to.

### We said yes to
- **A 48 px Spine instead of a 220 px sidebar.** Saves real estate; mode-switching is a one-tap operation, not a navigation tree.
- **Mode-first layouts.** Opinionated defaults beat infinite customization for 95% of users.
- **OKLCH color throughout.** Better perceptual uniformity, future-proof, browsers ship it.
- **Fraunces for display only.** Adds editorial gravity to a tool category dominated by all-sans interfaces.
- **The Control Plane as a Mode.** Engine internals are not a popup or a tab — they're a workspace.

### We said no to
- **Free-form panel docking.** Powerful but exhausting. We pick layouts users won't want to change.
- **A ribbon toolbar.** Wastes vertical space; teaches users nothing about keyboard paths.
- **Skeuomorphic 3D widgets.** No beveled gizmos, no glossy buttons. Flat, calibrated, instrumental.
- **A welcome screen with templates.** Templates live in the command palette. The first thing you see is a viewport.
- **Telemetry charts in the main UI.** Telemetry belongs in the Control Plane Mode. The Scene mode shows you the scene.

---

## 12 — Open questions

What this design does not yet answer, and where the next iteration should go.

1. **Multi-window.** How does Spine + Modes work when the user pops the viewport to a second monitor? Likely the popped window keeps its own Spine, but with only one mode lit.
2. **Collaboration cursors.** If two designers edit the same scene, where do their selections live in the gold-only color system? Likely tinted gold variants, but needs exploration.
3. **Plugin UI.** Third-party plugins need a place to live. Probably the Inspector as additional component cards, but the contract isn't defined.
4. **Mobile viewer.** Out of scope for v1, but the Spine pattern probably translates well to a tablet-sized viewer.
5. **Light theme.** Not planned. The deep field is load-bearing for the brand — a light variant would be a different product.

---

*End of document.*
