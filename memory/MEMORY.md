# Memory

## Current State

- **Branch**: `dev`
- **Build**: Clean (all crates compile, 0 errors)
- **Tests**: 533 passing, 32 ignored, 0 failures
- **Latest work (2026-05-08, follow-up)**: Save-As policy revert + universal asset tracking + entity→asset prefab drop
  - **Save-As is now single-action**: the title-bar sub-menu was reverted; "Save As…" dispatches the bare `"save_as"` action, which routes to `apply_save_as` and unconditionally uses `SerializationGoal::EditorInterchange`. The engine picks the goal — the user does not. Code paths that need a different goal still call `save_scene_dispatch_with_goal` directly (unchanged).
  - **Universal asset tracking**: `IndexBuilder` no longer drops files with unrecognized extensions. `asset_type_for_extension` now returns `Option<String>` and falls back to `Some(ext.to_lowercase())` for unknown extensions. Files with no extension are bucketed as `"blob"` (`EXTENSIONLESS_ASSET_TYPE`). New `should_skip_file` (pub) filters OS scratch (`.*` prefix, `.tmp` / `.swp` / `.bak` / `~` suffixes) — used by both the index builder and the hot-reload watcher (which previously filtered via the canonical extension allowlist). Net effect: any new file dropped under `assets/` flows into the VFS automatically; no code change needed to add a new asset kind.
  - **Hierarchy → asset panel drop = create prefab**: scene-tree row drag source already attaches a packed `EntityId` payload (high32=generation, low32=index). The asset browser now registers `ab-grid-drop` as the *last* interact rect over the tile area; `dnd_take_drop_payload` decodes the entity payload via `scene_tree::payload_is_entity` (anything that's not a `PREFAB_DRAG_TAG`-prefixed asset payload), looks up the entity name from `EditorState::scene_roots`, sanitizes it for filesystem use (`sanitize_for_filename`), and queues `pending_save_as_prefab_at: Option<(EntityId, String)>` (new EditorState field) with the path `<current_folder>/<sanitized_name>.kprefab`. `process_pending_save_as_prefab` now drains BOTH `pending_save_as_prefab` (right-click → file dialog) AND `pending_save_as_prefab_at` (drag-drop → no dialog, write directly through ProjectVfs). `pack_entity` / `unpack_entity` in `scene_tree.rs` are now `pub(crate)`.
- **Older work (2026-05-08)**: Three deferred-work loops closed
  - **Save-As strategy picker**: `File → Save As…` in `title_bar.rs` is now a sub-menu with one entry per `SerializationGoal` (Editor Interchange / Fastest Load / Smallest File / Human Readable / Long-Term Stability / Portable Binary). New action strings `"save_as:<goal>"` route through `apply_save_as_with_goal` → `save_scene_dispatch_with_goal`. The bare `"save_as"` action (used by the cmd palette) stays mapped to `EditorInterchange` for backwards compat. The redundant `"export_ron"` menu/dispatch was removed (covered by `save_as:human_readable`).
  - **Prefab UI**: subtree extraction added in `khora-data/src/scene/recipe_strategy.rs` (`serialize_subtree(world, root)` / `instantiate_subtree(world, bytes) -> EntityId`, BFS over `Children`, parent edges only emitted for in-subtree pairs so the root lands at world root on instantiation; one-test round-trip excludes outsider entities). Scene-tree row context menu now has "Save as Prefab…" (`EditorAction::SaveAsPrefab(EntityId)` → `EditorState::pending_save_as_prefab` → `commands::process_pending_save_as_prefab` writes `.kprefab` through ProjectVfs or `std::fs`). Asset browser registers `PrefabHandler` (`asset_type_name == "prefab"`, category "Prefabs", activation `ActivationKind::SpawnPrefab`). Prefab tiles attach a `u64` drag payload tagged with `PREFAB_DRAG_TAG = 0x4B48_5046_0000_0000` (`"KHPF"`); the viewport's `dnd_take_drop_payload` decodes the tag, looks up `EditorState::asset_entries[idx]`, and queues `pending_prefab_spawn`. `commands::process_pending_prefab_spawn` reads the recipe via `pvfs.asset_service.load_raw(uuid)` and calls `instantiate_subtree`. Both processors are called from `EditorApp::update` between `process_menu_actions` and `apply_edits`. Phase 5 (instance overrides, nested prefab linking) explicitly deferred.
  - **Runtime verify-integrity**: `AssetService::new` now takes `manifest: Option<PackManifest>`. When `Some`, both `load` and `load_raw` call `m.verify(uuid, &bytes)` between `io.load_bytes` and decode/return; mismatch surfaces as `anyhow::Error` with context "Asset integrity check failed". `khora-sdk/run_default::build_asset_service` reads `<exe-dir>/manifest.bin` when (a) PackLoader path is taken AND (b) `RuntimeConfig::verify_integrity == true`; missing/malformed manifest logs a warning and continues with `None` (never aborts startup). Dev-mode (`FileLoader`) ignores the flag with a single log line. New unit test in `service.rs` proves a corrupted byte triggers verify failure.
- **Older work (2026-05-03 evening)**: Editor UX fixes + DataSystem/services migration
  - **1A — Light variant switch**: inspector now shows a combo (`Directional`/`Point`/`Spot`) on `Light::light_type`. Walker (`render_object`) detects single-key serde-tagged enums against a registry (`crates/khora-editor/src/widgets/enum_variants.rs`) and replaces the payload with the variant's default JSON when the user picks another. Future enums are opt-in via that registry.
  - **1B — Remove component**: per-card trash button in the inspector; `PropertyEdit::RemoveComponent` variant; `ComponentRegistration::remove` function pointer auto-emitted by the `#[derive(Component)]` macro; dispatch in `apply_edits` via inventory lookup. Inherent components (Transform/Name/etc.) are filtered out as before.
  - **1C — Parent/Child via drag-and-drop**: `UiBuilder::dnd_drag_source` / `dnd_drop_target` primitives (egui-side wrap of `Response::dnd_set_drag_payload<u64>` / `dnd_release_payload`). Scene tree rows are sources + targets carrying packed `EntityId` (high32 = generation, low32 = index). Empty area at panel bottom = unparent target. `EditorAction::Reparent` + `EditorState::pending_reparent` + `ops::process_reparents` + `GameWorld::set_parent(child, Option<parent>)` with cycle prevention.
  - **2A — `DataSystem` reçoit `&ServiceRegistry`**: signature changée à `fn(&mut World, &ServiceRegistry)`. `transform_propagation_system` gains a thin `transform_propagation_entry(_, _services)` wrapper. Dispatcher `run_data_systems(world, services, phase)` updated everywhere.
  - **2B — `tick_maintenance` → DataSystem**: `EcsMaintenance` removed from `GameWorld`, now lives in `ServiceRegistry` as `Arc<Mutex<EcsMaintenance>>`. New `crates/khora-data/src/ecs/systems/ecs_maintenance.rs` runs in `Maintenance` phase.
  - **2C — `proj.sync_all` → DataSystem**: new `crates/khora-data/src/ecs/systems/gpu_mesh_sync.rs` runs in `PreExtract` phase. Engine no longer wires the sync call manually.
  - **2D + 2E — DEFERRED**: physics_lane and audio_lane still query World directly (broadphase, solver, spatial mixing). Migration requires a real `OutputDeck`-based writeback channel (TransformDelta, CollisionPairsOutput, AudioStateUpdates) plus commit DataSystems in `Maintenance`, plus end-to-end physics & audio regression tests. The Flow + bus infrastructure for input is ready (PhysicsFlow, AudioFlow stubs registered); the writeback design is the open work. Tracked as their own future PRs.
- **Older work (2026-05-03 morning)**: Substrate / Flow / AGDF refactor (Phases 0-9)
  - **P0** — `LaneBus`, `OutputDeck`, `TickPhase`, `DataSystemRegistration`, substrate dispatcher.
  - **P1** — `transform_propagation` migrated from `khora-lanes/scene_lane` → `khora-data/ecs/systems` as a `DataSystem` (PostSimulation phase). Resolves the inspector Transform bug by architecture; `scene_lane` deleted.
  - **P2** — `Flow` trait + `FlowRegistration` (inventory) + `flow_runner` in scheduler. `EngineContext` now carries `bus: &LaneBus` and `deck: &mut OutputDeck`. CLAD descent restored: agent invokes its own lane.
  - **P3** — `RenderFlow` publishes `RenderWorld` into `LaneBus`. `RenderWorldStore` and `extract_scene` deleted; render lanes migrated `Slot<RenderWorld>` → `Ref<RenderWorld>`.
  - **P4** — `UiFlow` deferred (atlas mutation by UiAgent + non-Clone `Box<dyn TextLayout>` need deeper redesign). `UiSceneStore` retained for now.
  - **P5** — `PhysicsFlow` with `adapt()` — first concrete AGDF realisation: detaches `RigidBody` outside `DETACH_RADIUS`, restores from internal stash inside `REATTACH_RADIUS` with hysteresis.
  - **P6** — `ShadowFlow` and `AudioFlow` stubs (publish stat views; no behaviour change yet).
  - **P7** — Orphan `CompactionLane` (`khora-lanes/ecs_lane`) deleted. Existing `EcsMaintenance` continues to drive compaction via `gw.tick_maintenance()`.
  - **P8** — All asset decoders moved from `khora-lanes/asset_lane/loading` → `khora-io/src/asset/decoders/`. `*LoaderLane` types renamed to `*Decoder`. `khora-lanes` deps for image/mesh/audio/font removed.
  - **P9** — `.agent/architecture.md`, `rules.md`, `conventions.md` updated with the Substrate Pass doctrine and 4 new hard rules.
- **Older work**: SAA Lifecycle Refactoring (former Phases 1-8)
  - **Phase 1 — Agent trait cleanup**: Removed `update()`, added `on_initialize()`, `execute()` receives `&mut EngineContext`. Only 4 agents (Render, Physics, UI, Audio). Added `execution_timing()` to Agent trait. ✅
  - **Phase 2 — ExecutionPhase + EngineMode**: Renamed old `ExecutionPhase` (Boot/Menu/Simulation/Background) → `EnginePhase` → deleted. New `ExecutionPhase` (Init/Observe/Transform/Mutate/Output/Finalize) for frame pipeline. `EngineMode` (Editor/Playing) for engine state. ✅
  - **Phase 3 — Lane lifecycle**: Added `prepare()` and `cleanup()` to Lane trait (default no-op). Removed `kind()` method. ✅
  - **Phase 4 — khora-io crate**: New crate for I/O services:
    - `VirtualFileSystem` from `khora-core` ✅
    - `AssetIo` trait + `FileLoader` (dev) + `PackLoader` (release) ✅
    - `AssetDecoder` trait from `khora-lanes` (no Lane bound) ✅
    - `DecoderRegistry` + `AssetService` from `khora-agents` ✅
    - `SerializationStrategy` from `khora-lanes` (no Lane bound) ✅
    - 3 strategies + `SerializationService` from `khora-lanes`/`khora-agents` ✅
  - **Phase 5 — Scene workflow**:
    - Editor auto-loads `default.kscene` on project open ✅
    - Editor creates default scene if none exists ✅
    - Double-click `.kscene` in asset browser loads scene ✅
    - `EditorState.pending_scene_load` for async scene loading ✅
  - **Phase 6 — Component serialization + Add Component UI**:
    - `#[derive(Component)]` macro generates `SerializableX` + `From` conversions ✅
    - `#[component(skip)]` attribute for non-serializable fields (GPU handles) ✅
    - `#[component(no_serializable)]` for unit structs handled manually ✅
    - `inventory::submit!` for ALL 25 components (was only 6) ✅
    - "Add Component" button in Properties Panel ✅
    - `add_component_to_entity()` in ops.rs ✅
    - Scene tree rename supports Enter/Escape ✅
  - **Phase 7 — SAA Scheduler**:
    - `ExecutionScheduler` in `khora-control` — hot-path orchestrator ✅
    - `BudgetChannel` — unidirectional cold → hot thread communication ✅
    - `EnginePlugin` — extensible hooks per ExecutionPhase ✅
    - `AgentDependency` system with Hard/Soft/Parallel + conditions ✅
    - `execution_timing()` on all 4 agents (Render: Observe/Output/Critical, Physics: Transform/Critical, UI: Observe/Output/Important, Audio: Transform/Important) ✅
    - SDK integrated Scheduler — `EngineState.scheduler` (private), `EngineState.context`, `EngineState.services` ✅
    - `AppContext.services` → `Arc<ServiceRegistry>` ✅
    - DCC cold thread → BudgetChannel → Scheduler ✅
    - Frame loop uses `scheduler.run_frame()` instead of `dcc.execute_agents()` ✅

## Known Issues

- Vulkan semaphore validation errors still present at runtime
- Object jittering when moving camera — may be camera matrix precision or shadow-related
- egui-wgpu crate incompatible with wgpu 28.0 — custom renderer in khora-infra
- Editor unused import warnings after prelude cleanup (cosmetic, not errors)
- `InspectedEntity` snapshot extraction only populates core fields — newer fields always false/None
- **All three Phase-3/4 compromises resolved (2026-05-03)**:
  - Editor camera → `EditorViewportOverride` service (in `khora-data/src/render/editor_view.rs`); `RenderFlow` reads it as fallback when no active scene `Camera` exists.
  - Shadow patches → `ShadowEntries` typed slot in `OutputDeck` (in `khora-data/src/render/shadow_outputs.rs`); `shadow_pass_lane` writes, `lit_forward_lane` reads.
  - UI pipeline → `UiFlow` (with `&ServiceRegistry` access) does extract+text-layout; atlas allocation moved out of `ExtractedUiNode` into a separate `UiAtlasMap` owned by `UiAgent`. `UiSceneStore`, `extract_ui_scene`, `layout_ui_text` deleted.
- `Flow` trait now takes `&ServiceRegistry` in `select` / `adapt` / `project`. `select` got a default empty implementation (most flows don't need one).
- **Shadow math migrated from lane to Flow (2026-05-03)**: `calculate_shadow_view_proj` (CSM, Spot perspective) lifted from `ShadowPassLane` into `ShadowFlow::project`. The lane now reads `Ref<ShadowView>` from `LaneContext` and only handles atlas allocation + GPU passes. `primary_view(world, services)` in `khora-data/src/render/mod.rs` is the shared camera resolver used by both `RenderFlow` and `ShadowFlow` (consults `EditorViewportOverride` on fallback). Index alignment between `RenderWorld.lights` and `ShadowView.matrices` is preserved by both flows iterating the same filtered query in the same order.

## Architecture Decisions

- **12 crates** in workspace: core, data, io, lanes, control, agents, infra, telemetry, macros, plugins, sdk, editor
- **SAA Scheduler** (private in SDK): Orchestrates agent execution per frame based on phase, priority, dependencies, and budget pressure
- **ExecutionPhase** (Init/Observe/Transform/Mutate/Output/Finalize): Frame pipeline stages — agnostic of subsystems
- **EngineMode** (Editor/Playing): Engine state — determines which agents are active
- **EnginePlugin**: Extensible hooks that inject into the frame pipeline at specific phases
- **BudgetChannel**: Unidirectional crossbeam channel from DCC cold thread to Scheduler hot thread ("last wins" semantics)
- **Agent timing**: Each agent declares `ExecutionTiming` (allowed phases, priority, importance, dependencies)
- **Lane lifecycle**: `prepare()` → `execute()` → `cleanup()` with shared `LaneContext`
- **Agent vs Service rule**: 4 agents (Render, Physics, UI, Audio) — non-GORNA uses services in `khora-io`
- **khora-io**: Dedicated crate for I/O services — separates data plane from control plane
- **Component serialization**: `#[derive(Component)]` generates `SerializableX` + `From` + inventory registration
- **ECS maintenance**: `EcsMaintenance` in `GameWorld.tick_maintenance()` — not an Agent
- **GORNA**: Dynamic agent budget negotiation with thermal/battery multipliers
- **SDK is a facade**: Scheduler, BudgetChannel, and EnginePlugin are internal — users only see `Engine` API
