# Product

## Register

product

## Users

Two audiences, one design language:

- **Engine contributors** working *on* Khora — Rust systems programmers reading the editor's Control Plane to watch GORNA budget negotiation, agent health, and the frame descent in real time. They live in dense, information-rich panels and expect the tool to disappear into the work.
- **Game developers** working *with* Khora — building scenes in the editor (hierarchy, viewport, inspector, assets) and managing projects/engine versions from the Hub. They arrive fluent in Unity, Godot, Blender, and expect equivalent muscle-memory affordances (transform gizmo, view-orientation gizmo, dockable panels, Cmd+K).

Both are expert users in a focused task, not first-time visitors. The interface serves the task; it never performs.

## Product Purpose

Khora is an experimental Rust game engine with a Symbiotic Adaptive Architecture (subsystems negotiate frame budgets via GORNA). Two front-ends ship it:

- **Editor** (`khora-editor`) — the scene authoring + engine-introspection app. Scene mode (hierarchy / viewport / inspector / console / assets) and Control Plane mode (DCC metrics, agent negotiation, frame descent).
- **Hub** (`khora-hub`) — the project manager / launcher: create projects, install and switch engine versions, connect GitHub.

Success = a contributor or game dev fluent in the category's best tools sits down and trusts the interface immediately, with nothing subtly off. The engine's uniqueness (adaptive, self-watching) is made *legible* by the Control Plane, not decorated.

## Brand Personality

Precise, technical, restrained, engineered. Three words: **instrument, not toy.** The tone matches the engine's own voice — a Rust systems programmer's: short, exact, backed by real numbers. Confidence through clarity and density, never through ornament. The navy/silver palette with a single gold selection accent reads as a measuring instrument or a mastering console: calm dark surfaces, silver structure, one point of light where the user's attention is.

## Anti-references

- **Consumer/playful game-maker chrome** (Roblox Studio, Scratch, rounded candy toolbars). Khora is an instrument.
- **Generic SaaS dashboard** — hero-metric templates, big-number-small-label tiles with gradient accents, identical card grids. The Control Plane shows real telemetry densely, not marketing KPIs.
- **Boxes-in-boxes inspectors** — nested bordered cards for every component. The inspector is flat, dense, collapsible; structure comes from typography and spacing, not borders.
- **Cream/warm-neutral "editorial" palettes**, glassmorphism, gradient text, decorative motion, side-stripe accents. None of it.
- **Rainbow accents** — color is semantic only (axis X/Y/Z, log levels, agent tiers, gold selection). No decorative color.

## Design Principles

1. **Work is hero, chrome retreats.** The viewport, the scene data, the telemetry are the content. Titlebar, spine, and status bar are quiet dark structure around them.
2. **One design system, two apps, zero drift.** Editor and Hub share a single token source and widget vocabulary. A new panel or a future tool (asset cooker) is automatically on-brand because it is built from the same parts.
3. **Density is a feature, legibility is non-negotiable.** Pack information (agent lists, log grids, property rows) but keep every text run at ≥4.5:1 contrast and every control at its full state set.
4. **Every control has all its states.** default / hover / focus / active / disabled / selected / loading / error — never ship half. Skeletons for loading, teaching empty states, not spinners and "nothing here".
5. **Make the adaptive engine legible.** The Control Plane's job is to render GORNA negotiation, budgets, and the frame descent so a contributor can *read* a frame. Truth over polish; show the real numbers.
6. **Earned familiarity over novelty.** Standard affordances (transform gizmo, view gizmo, Cmd+K, dock tabs, breadcrumbs) work exactly as expert users expect. Surprise is saved for nothing.

## Accessibility & Inclusion

- Body text ≥ 4.5:1 against its surface; large/bold text ≥ 3:1. Silver-on-navy and all four text tiers verified against their surfaces.
- Color is never the sole signal: log levels pair an icon with the row tint; axis fields pair the X/Y/Z letter with the color; agent tiers pair a letter badge with position. Safe for color-vision deficiency.
- Motion is 150–250 ms, conveys state only (no orchestrated load sequences); honor reduced-motion with instant/crossfade fallbacks in any animated affordance.
- Icon-only controls (spine, transport, icon buttons) carry tooltips/labels.
