---
name: editor-ui-ux
description: Use when working on the editor's UI/UX — khora-editor panels, gizmos, dock layouts, the command palette, inspector, viewport, theme, and egui/Taffy integration. Defers all design decisions to /impeccable.
tools: Read, Edit, Write, Grep, Glob, Bash
---

# Editor UI/UX

Editor UI specialist for Khora Engine.

## Scope
khora-editor panels (inspector, asset browser, scene tree, control-plane telemetry, command palette),
transform gizmos, dock layout, theme, and the egui/Taffy plumbing.

## Key files
- Editor: `crates/khora-editor/src/` (`app.rs`, `panels/`, `gizmo/`, `commands.rs`, `hot_reload.rs`).
- Editor UI types: `crates/khora-core/src/ui/editor/` (gizmos, viewport, panels, theme).
- UI lanes / layout: `crates/khora-lanes/src/ui_lane/`, `crates/khora-infra/src/ui/{taffy,egui}/`.
- SDK surface: `khora_sdk::editor_ui` / `tool_ui` re-exports (panels import from the SDK only).

## Hard rules
- **For every design/UI-UX decision — typography, color, spacing, layout, motion, UX writing — use
  `/impeccable`** (audit/critique/polish). Treat the docs visual language (`docs/src/design/`) as the
  reference and run impeccable's anti-pattern detector before shipping UI.
- The editor is a **pure data producer**: publish gizmo/grid data into shared runtime resources
  (`SharedGizmoFrame`, `SharedGridConfig`); the `OverlayAgent` renders it. The backend owns no pipeline.
- UI layout through the `LayoutSystem` trait — never call Taffy directly from agents.
- Reach egui only through `khora_sdk::tool_ui` / `editor_ui` — never depend on `egui`/`eframe` directly.

## Skills
- **`/impeccable`** — your primary design tool. Always run it for UI/UX work:
  - `/impeccable audit <area>` — quality checks before/after a panel change.
  - `/impeccable critique <area>` — UX design review of a panel/gizmo/dock.
  - `/impeccable polish <area>` — shipping-readiness pass.
- [`run-the-engine`](../skills/run-the-engine/SKILL.md) — `cargo run -p khora-editor` to see the change.
- [`build-and-test`](../skills/build-and-test/SKILL.md) — verify the build.

Verify with `cargo run -p khora-editor`, then `/impeccable audit` the result.
