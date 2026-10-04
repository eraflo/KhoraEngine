# Editor UI/UX — reference

Domain knowledge for the Khora editor UI. Consulted during Research (dispatch the `codebase-*` subagents
to apply it to concrete files). For any design decision, use `/impeccable`.

## Scope
khora-editor panels (inspector, asset browser, scene tree, control-plane telemetry, command palette),
transform gizmos, dock layout, theme, and the egui/Taffy plumbing.

## Key files
- Editor: `crates/khora-editor/src/` — `app/` (`EditorApp`, play-mode → engine-mode), `panels/`
  (`workbench.rs` dock host, `scene_tree/`, `asset_browser/`, `viewport/`, `control_plane/`, console,
  command palette), `chrome/` (title bar, spine, status bar — all branding), `widgets/` (editor-only
  widgets; `widgets/inspector/` = cards, tabs, field `walker.rs`, `instance_band.rs`, add-component),
  `ops/` (pure ECS ops: hierarchy, inspect, spawn, scene tree, `prefab_overrides/`), `commands/` (drain
  `EditorState` pending actions: scene, prefab, material, asset ops), `scene_io/` (project-relative save/
  load, Play snapshot/restore), `gizmo/`, `project_vfs.rs`, `hot_reload.rs`.
- Editor UI types: `crates/khora-core/src/ui/editor/` (gizmos, viewport, panels, theme).
- UI lanes / layout: `crates/khora-lanes/src/ui_lane/`, `crates/khora-agents/src/ui_agent/`, `crates/khora-infra/src/ui/{taffy,egui}/`.
- SDK surface: `khora_sdk::editor_ui` / `tool_ui` re-exports (panels import from the SDK only).

## Prefab overrides (inspector)
- Types (`khora-core/src/ui/editor/state.rs`): `InspectedEntity::prefab: Option<InspectedPrefab>`
  (`root`, `prefab`, `prefab_path`, `is_root`, `components: Vec<ComponentOverride>`, `removed`,
  `prefab_json`, `override_count()`), `ComponentOverride { type_name, added, fields }`,
  `PrefabApplyScope::{Field, Component, Instance}` queued as `EditorState::pending_prefab_apply`.
- Detect / revert: `ops/prefab_overrides/mod.rs` (`json_overrides`, `reverted`, `revert_instance`).
  Apply: `commands/prefab.rs` → `khora_data::scene::apply_to_prefab`. Instance-wide revert:
  `revert_prefab_instance` in `app/mod.rs`.
- UI: `widgets/inspector/instance_band.rs` (one line under the header: prefab link + "Apply all" /
  "Revert all"), `card.rs` (component Apply/Revert, `Icon::ApplyToPrefab` / `Icon::Revert`), `walker.rs`
  + `tabs.rs` (per-field Revert, "Revert to prefab" menu).
- **Never overrides:** the instance root's `Transform.translation` / `rotation` (they place the instance)
  and the `PrefabInstance` link itself (also the root's `Parent`) — excluded in both the editor
  (`PLACEMENT`, `LINK`) and `prefab/overrides.rs`.

## Hard rules
- **For every design/UI-UX decision — typography, color, spacing, layout, motion, UX writing — use
  `/impeccable`** (audit/critique/polish). Treat the docs visual language (`docs/src/design/`) as the
  reference and run impeccable's anti-pattern detector before shipping UI.
- The editor is a **pure data producer**: publish gizmo/grid data into shared runtime resources
  (`SharedGizmoFrame`, `SharedGridConfig`); the `OverlayAgent` renders it. The backend owns no pipeline.
- UI layout through the `LayoutSystem` trait — never call Taffy directly from agents.
- Reach egui only through `khora_sdk::tool_ui` / `editor_ui` — never depend on `egui`/`eframe` directly.
- The editor runs in `EngineMode::Custom("editor")` while editing; Play/Pause map to `EngineMode::Playing`
  (`engine_mode_for`). Never run gameplay while editing.
- Play captures a `FastestLoad` snapshot in memory; Stop restores it atomically with the same
  `PersistentId`s. Scenes on disk are `EditorInterchange` (compact).

## Skills
- **`/impeccable`** — the primary design tool. Always run it for UI/UX work:
  - `/impeccable audit <area>` — quality checks before/after a panel change.
  - `/impeccable critique <area>` — UX design review of a panel/gizmo/dock.
  - `/impeccable polish <area>` — shipping-readiness pass.
- [`run-the-engine`](../skills/run-the-engine/SKILL.md) — `cargo run -p khora-editor` to see the change.
- [`build-and-test`](../skills/build-and-test/SKILL.md) — verify the build.

Verify with `cargo run -p khora-editor`, then `/impeccable audit` the result.
