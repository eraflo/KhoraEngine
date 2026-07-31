# Knowledge — Working Memory

Living state of the engine. Update when state changes (build status, latest work, known issues).
Deep history lives in git and `docs/plans/`.

## Current state
- **Branch**: `dev`
- **Build**: clean (all crates compile, 0 errors); clippy 0 errors.
- **Tests**: ~866 passing, ~29 ignored, 0 failures. Treat the live `cargo test --workspace` count as truth.

## Latest work (2026-07-31) — Audit UI éditeur + provenance des composants
- **Audit complet de l'UI éditeur** : `docs/research/2026-07-31_editor-ui-audit.md` — 12 sections,
  ~90 constats identifiés (`H1`, `Q2`, `X8`…), menés en lecture de code **et** en exécution réelle à
  trois largeurs de fenêtre. Plan de correction en 7 phases. Défauts structurels : aucun panneau ne
  scrolle (`scroll_area` a zéro appelant), `Interaction` n'a pas de `focused` et `UiBuilder` aucune
  API clavier, `last_response` non renseigné par les champs de saisie (⇒ `Entrée`/`Échap` morts dans
  la palette), 22 des 33 widgets de `khora-tool-ui` sans appelant.
- **`ComponentProvenance` (nouvel axe, `khora-data/src/ecs/registry.rs`)** — orthogonal à
  `SemanticDomain` : celui-ci dit *qui consomme* la donnée, la provenance dit *qui a le droit de
  l'écrire*. 4 variantes encodant deux bits (offert à l'auteur / copié à la duplication) :
  `Authored` (défaut) · `ToolAuthored` (`Prefab`, `Parent`) · `Derived` (`GlobalTransform`,
  `Children`) · `Runtime` (`PhysicsDebugData`). Déclaré via `#[component(provenance = …)]`.
  Remplace quatre encodages ad hoc concurrents (`no_serializable`, `#[component(skip)]`,
  `INHERENT_COMPONENTS`, la liste en dur de `duplicate_entity`).
- **`serialize_all_components` filtre désormais sur la provenance**, et les **4 chemins de
  sérialisation** (subtree/prefab, recipe monde, messagepack, definition) passent par lui — ils
  itéraient l'inventaire en direct. Conséquence corrigée : les `.kprefab` et les scènes
  embarquaient `Children`/`GlobalTransform` avec les **EntityId de la source**.
- **`link_parent_child`** (`scene/registry.rs`) : les 3 gestionnaires `SceneCommand::SetParent`
  n'ajoutaient que `Parent` ; l'index inverse `Children` venait du composant sérialisé (donc faux).
  Ils maintiennent maintenant les deux côtés, comme `GameWorld::set_parent`.
- **`duplicate_entity` réécrit** (`khora-editor/src/ops.rs`) : passe par le round-trip
  `serialize_subtree`/`instantiate_subtree` au lieu d'une liste de 8 composants en dur. Corrige la
  perte de `Tag`, des composants utilisateur, du `Parent` et de tout le sous-arbre. 2 tests de
  régression.
- **« + Add Component » / cartes Inspector** : filtrage via `is_author_facing` (provenance + domaine
  `Ui` masqué hors workspace Canvas) au lieu de la liste de chaînes `INHERENT_COMPONENTS`.
  Vérifié à l'écran : les buckets `UI` et `Other` (où vivait `Prefab`) ont disparu.
- **Bug de macro corrigé** (`khora-macros`) : `parse_nested_meta` avortait sur la première clé
  `key = value` non consommée et l'erreur était avalée par `let _ =`, donc une seconde clé n'était
  jamais lue. Les parseurs `domain`/`provenance` sont fusionnés en une passe et `no_serializable`
  est durci.
- Vérif : `cargo test --workspace` 940 passed / 0 failed ; `cargo clippy --workspace` clean ;
  éditeur lancé plusieurs fois, sortie propre (code 0), aucun panic.

## Earlier work (2026-07-18) — Audit remediation (robustness + dead-code sweep)
- Plan/recherche : `docs/plans/2026-07-18_codebase-audit-remediation.md`, `docs/research/2026-07-18_codebase-audit.md`.
- **Results faillibles jetés** (`let _ =`) désormais gérés : `write_buffer` (`forward_plus_lane`, pattern
  `if let Err → log::error`), `add_component` (`game_world`, `asset_resolver`, `projection` → logués),
  `device.poll` (`wgpu/device.rs`). `// SAFETY:` ajouté à `world.rs` (page-migration).
- **Locks poison-safe workspace-wide** : ~30 sites `.lock()/.read()/.write().unwrap()` → idiome
  `.unwrap_or_else(|e| e.into_inner())` (recovery, ne crashe jamais) ; `text.rs::flush` propage via `?`.
  Helpers existants : `khora_core::lane::lock` + macro `lock_or_log!` (khora-lanes).
- **Hygiène** : `command.rs` profiler pass index → fallback logué ; `unreachable!()` gardés auto-documentés.
- **Code mort supprimé** : `physics_lane/native_lanes.rs` (orphelin, violait CLAD R2) + son wiring
  `CollisionPairs` mort dans `engine.rs` ; module `ui_lane`/`StandardUiLane` + champ `layout_lane` de
  `UiAgent` (chemin jamais exécuté, violation CLAD R2 latente éliminée) ; imports morts `lit_forward_lane`.
- **Unification statut agent (Option A)** : les 5 agents ne portent **plus d'état par-frame**. Le
  scheduler mesure le temps d'`execute` et l'écrit dans un `AgentFrameStatusMap`
  (`Arc<RwLock<HashMap<AgentId, AgentFrameStatus>>>`, dans `runtime.resources`, défini
  `khora-core/control/gorna.rs`). `report_status` lit ce temps via le free fn `measured_frame_time_ms`
  (pas de méthode inhérente, RULES §8) et dérive `health_score` du `time_budget` retenu. `is_stalled`
  retiré (`false`) : le scheduler ne peut pas distinguer « crash interne » d'un skip volontaire → GORNA
  garde les triggers pression + health-degraded, perd le détecteur crash (qui reposait sur l'auto-report).
  `AgentStatus` inchangé ; editor + GORNA non modifiés. draws/tris de l'editor = `GpuReport`/MonitorRegistry.
- Vérif : `cargo test --workspace` 866 passed / 0 failed ; `cargo clippy --workspace` clean ; run editor
  12 s sans panic ni erreur de validation Vulkan.

## Latest work (2026-07-15) — Asset explorer + stable asset identity
- **Stable asset UUIDs via a registry** (`khora-io/src/asset/id_registry.rs`, `AssetIdRegistry`): UUIDs were
  always `new_v5(rel_path)`; now that's only the *default*. `<project>/.khora/asset-registry.ron` (RON, one
  `(uuid, path)` per line, sorted by UUID, atomic write) can **freeze** an identity. **Lazy freeze**: no entry ⇒
  `new_v5(path)` (old projects/tests unaffected); a rename/move freezes the *current* UUID so references
  (`MeshRef::Asset`/`MaterialRef`/texture slots — stored as raw UUID bytes) never break, on disk or in the open
  scene, with zero rewriting. Read side is engine (`IndexBuilder::with_registry`, `PackBuilder` both resolve
  through it ⇒ dev/release parity); **only the editor writes** it (`ProjectVfs`). Registry lives at project root
  (outside `assets/`) ⇒ never scanned/watched/packed.
- **Real file explorer** (`khora-editor/src/panels/asset_browser/`): generic per-tile/folder/empty context menus
  (Open, Reveal, Rename [inline], Duplicate, Delete→OS recycle bin via `trash` crate), wired header buttons,
  empty folders shown (`ProjectVfs::list_dirs` + `EditorState::asset_dirs`), epoch-based rescan
  (`asset_epoch`, replaces the count-only key), generic drag payload (`ASSET_DRAG_TAG`, was prefab-only).
  Drag a tile onto a folder = move; onto the viewport = instantiate (mesh spawns `MeshRef::Asset` at the
  unprojected ground-plane drop point via `UiBuilder::pointer_position` + `EditorCamera::screen_to_ray`, prefab
  instantiates, scene loads, texture/material assigns to the selected entity).
- File ops routed through new `ProjectVfs::{create_folder,rename_asset,move_asset,delete_to_trash,duplicate_asset}`
  + drained by `commands::process_pending_asset_file_ops` / `process_pending_spawn_mesh_asset` /
  `process_pending_assign_texture`. New deps: `trash`, `walkdir` (editor).

## Latest work (2026-06-11) — Adaptive loop closure
- **PID recovery loop closed** (`khora-control/src/service.rs`): the DCC remembers the
  `global_budget_multiplier` at last budget issuance and re-arbitrates when the frame-time PID drifts
  it by more than `PID_RENEGOTIATE_DELTA` (0.05) — pressure heuristics drive downgrades, this drift
  trigger drives *recovery* (agents upgrade again once measured frame time settles under the setpoint).
- **Empirical cost calibration** (`khora-control/src/gorna/mod.rs`): `GornaArbitrator::arbitrate` takes
  per-agent measured costs (CostModel forecast at current workload, fallback `latest_ms`); during
  negotiation each agent's quoted estimates are rescaled so the current-strategy option equals the
  measurement (factor clamped `[0.25, 4.0]`) — the fit reasons in measured ms, not static quotes.
- **Three real shadow tiers**: new `MediumShadowsLane` (1024² × 4-layer 2D + 256² × 4-cube, ≈22 MiB).
  Mapping: HighPerformance→Standard (2048²+512², ≈88 MiB), Balanced→Medium, LowPower→LowRes
  (512²+128², ≈5.5 MiB). Honest per-tier VRAM quotes, each tier gated on the VRAM constraint; LowRes
  always offered as the floor.
- **Per-domain change epochs + Flow view caching** (`khora-data`): `World::domain_epoch` /
  `World::instance_id`; opt-in `Flow::cache_key`; `register_flow!` (`run_flow_cached`) republishes the
  previous View on a key hit. Cached: Audio, Render, Shadow (Render/Shadow fold a bit-level fingerprint
  of the editor viewport override). Uncached: Ui (surface size / hot-reload fonts have no signal),
  Physics (mutates every simulated frame). Representation-only — cached View is bit-identical.
- **Sequential budget semantics documented** (`khora-control/src/scheduler.rs` rustdoc + `08_gorna.md`):
  agents run sequentially in priority order, so GORNA budgets are exclusive per-agent time slices
  (sum-of-costs fitting); parallel execution (stub) will require a critical-path model.

## Earlier work (2026-05-19) — WGSL migration finalization
- **Shadow agent**: `ShadowStrategy::from_strategy_id` made `pub`; tests cover StrategyId→ShadowStrategy
  mapping (`LowPower→LowRes`, `HighPerformance/Balanced→Standard` — since superseded by the Medium tier,
  see Latest work) and `apply_budget`.
- **Forward+ shadows**: `ForwardPlusLane` reads `ShadowFrame` from the deck; group 3 = shadow bindings,
  group 4 = F+ tile/light data + `shadow_view_projs`. Directional/spot 2D PCF + point-light cube shadows
  → full parity with `LitForwardLane`.
- **OverlayAgent**: new agent owning post-main-render `LaneKind::Render` lanes, runs in `OUTPUT` after
  `Renderer` (Hard dep). Lanes run in parallel, order `Grid → Emissive → Wireframe → Gizmo`.
- **GizmoLane / GridLane**: editor/debug overlays via `Arc<Mutex<…>>` shared runtime resources
  (`SharedGizmoFrame`, `SharedGridConfig`), `LoadOp::Load`. The backend (`khora-infra`) no longer owns
  any render pipeline — the editor is a pure data producer.
- **StandardPbrLane**: alternative `RenderAgent` strategy (Cook-Torrance GGX), shares the
  `khora::std/lighting/shadow` lib modules and the group-3 contract.
- **Legacy WGSL cleanup**: 14 duplicate root `.wgsl` files deleted; canonical sources under
  `shaders/pipelines/` + `shaders/lib/`, referenced via `ShaderRegistry` + `naga_oil #import`.
- **Bugfix — 5-bind-group Forward+ pipeline**: F+ briefly used 5 groups; `max_bind_groups == 4` made the
  pipeline invalid (scene stopped rendering). Fixed by the canonical 4-bind-group convention
  (`conventions.md §10`): group 3 = whole lighting domain.
- Remaining render-lane TODOs (no separate tracking doc — read the in-code `// TODO`s): Emissive/Wireframe
  `execute` draw bodies (`emissive_lane.rs`, `wireframe_lane.rs`), LitForward CLAD refactor, GORNA `Custom`
  strategy support.

## Earlier milestones (condensed; see git history)
- **Substrate / Flow / AGDF refactor**: `LaneBus`/`OutputDeck`/`TickPhase`/`DataSystemRegistration`;
  `transform_propagation` moved to a `DataSystem`; `Flow` trait + `register_flow!`; `RenderFlow`/
  `ShadowFlow`/`PhysicsFlow`/`AudioFlow`; asset decoders moved to `khora-io`; `Flow::adapt` removed.
- **SAA lifecycle refactor**: `Agent` trait cleanup (`on_initialize`/`execute`), new `ExecutionPhase`
  (Init/Observe/Transform/Mutate/Output/Finalize) + `EngineMode`, `ExecutionScheduler` + `BudgetChannel`
  + `EnginePlugin`, `khora-io` crate split out.
- **Editor**: component serialization via `#[derive(Component)]`, Add/Remove Component UI, prefab
  extract/instantiate (`.kprefab`), drag-and-drop reparenting, Save-As goal picker.

## Open / deferred work (verified against code, 2026-06-04)
- **`EmissiveLane` / `WireframeLane` `execute` are no-ops** — pipelines compose at init but the draw bodies
  are TODO (`emissive_lane.rs`, `wireframe_lane.rs`) pending gating data (`MaterialKind::Emissive`
  flag / a debug flag).
- **Minor TODOs**: GPU VRAM capacity detection + dynamic adapter name (`wgpu/device.rs`), GPU timestamp
  writes (`wgpu/command.rs`), Taffy uses a hardcoded 1920px viewport width (`ui/taffy/taffy_layout.rs`).

> Verify the live state before asserting — these are code-grounded as of the date above, not runtime claims.
> Resolved (do not re-list as issues): Vulkan semaphore errors are handled by the single-acquire frame
> lifecycle (`wgpu/device.rs`); egui↔wgpu-28 is solved by the custom `EguiWgpuRenderer` in `khora-infra`.
> The orphan `physics_lane/native_lanes.rs` (experimental broadphase/solver that queried the World
> directly) was **removed** rather than migrated; the real physics path routes through `PhysicsProvider`
> + the `physics_world_writeback` DataSystem.

## Architecture decisions
See [`decisions.md`](./decisions.md). Crate count is **16** (13 `khora-*` + sandbox + xtask + hub);
older notes saying 11/12 were stale and omitted `khora-runtime`.
