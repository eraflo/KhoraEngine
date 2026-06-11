# Knowledge — Working Memory

Living state of the engine. Update when state changes (build status, latest work, known issues).
Deep history lives in git and `docs/plans/`.

## Current state
- **Branch**: `dev`
- **Build**: clean (all crates compile, 0 errors); clippy 0 errors.
- **Tests**: ~586 passing, ~35 ignored, 0 failures. Treat the live `cargo test --workspace` count as truth.

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
- Remaining TODOs tracked in `docs/plans/render-lanes-followups.md` (visual validation,
  Emissive/Wireframe `execute` bodies, real PBR material struct, LitForward CLAD refactor, GORNA
  `Custom` strategy support).

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
- **`physics_lane` reads the World directly** — `physics_lane/native_lanes.rs` still uses `world.query`/
  `world.get_many_mut` for broadphase and the solver, instead of an `OutputDeck` writeback channel. This is
  the deferred CLAD-purity migration (audio already routes through the mix bus). Flow/bus input side is ready.
- **`EmissiveLane` / `WireframeLane` `execute` are no-ops** — pipelines compose at init but the draw bodies
  are TODO (`emissive_lane.rs:258`, `wireframe_lane.rs:257`) pending gating data (`MaterialKind::Emissive`
  flag / a debug flag). Tracked in `docs/plans/render-lanes-followups.md`.
- **Minor TODOs**: GPU VRAM capacity detection + dynamic adapter name (`wgpu/device.rs`), GPU timestamp
  writes (`wgpu/command.rs`), Taffy uses a hardcoded 1920px viewport width (`ui/taffy/taffy_layout.rs`).

> Verify the live state before asserting — these are code-grounded as of the date above, not runtime claims.
> Resolved (do not re-list as issues): Vulkan semaphore errors are handled by the single-acquire frame
> lifecycle (`wgpu/device.rs`); egui↔wgpu-28 is solved by the custom `EguiWgpuRenderer` in `khora-infra`.

## Architecture decisions
See [`decisions.md`](./decisions.md). Crate count is **16** (13 `khora-*` + sandbox + xtask + hub);
older notes saying 11/12 were stale and omitted `khora-runtime`.
