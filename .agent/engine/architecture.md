# Khora Engine — Architecture Brief (engine profile)

Dense reference for AI agents. Full narrative in [`../../docs/src/concepts/clad.md`](../../docs/src/concepts/clad.md).

- Document — Khora Architecture Brief v3.0
- Status — Active

---

## 0 — Use codegraph first

A **codegraph** MCP server indexes every symbol, edge and file in the workspace (~1 s behind writes).
Query `codegraph_explore` with a question or the symbol names before grepping or reading whole files:
it returns their source and the call paths between them. Use Read/Grep only for what it did not cover.

## 1 — CLAD layers

```
SAA (the why)                       CLAD (the how)
─────────────────────────           ──────────────────────────
Dynamic Context Core (DCC)          khora-control
GORNA Protocol + arbiter            khora-control (protocol types in khora-core)
Intelligent Subsystem Agents        khora-agents (the one LaneAgent) + khora-lanes (families)
Adaptive Game Data Flows (AGDF)     khora-data (CRPECS, Flows, residencies)
Semantic Interfaces & Contracts     khora-core
I/O Services + trust boundary       khora-io
Observability & Telemetry           khora-telemetry
Hardware & OS Interaction           khora-infra
```

CLAD is the **command path** of a frame:

```
Control ──► Agent ──► Lane ──► Data (via typed Views)
       envelope  chooses a row   reads bus / writes deck
```

```
Pass A — Substrate (Scheduler):   DataSystem invariants → Flows (and HostViews) publish Views into the LaneBus
Pass B — CLAD descent:            lend allocations → swap staged layers → wake agents for events →
                                  run each family's installed layer at its place, in waves
                                  (LaneInputs from bus/deck/runtime → Lane::record → LaneOutputs)
Pass C — I/O boundary (Engine):   submit the FrameGraph's passes; Maintenance DataSystems drain the OutputDeck
```

The Substrate Pass (`khora-control/src/substrate/`) is the Data layer maintaining and projecting itself,
called by the scheduler because it owns the tick. **Only agents compete for the frame budget**; AGDF
(data layout adaptation, `khora-data/src/ecs/layout/`) self-optimises inside `khora-data` and is only
observed by the DCC. Detail: `docs/src/concepts/the-frame.md`, `agdf.md`, `change-detection.md`.

## 2 — Crates (17 workspace members)

The tiers and the allowed dependency edges are in [`RULES.md`](./RULES.md) §3, enforced by
`xtask/tests/crate_graph.rs`. Members: the 14 `khora-*` below (not `khora-macros`, a path crate),
`sandbox`, `xtask`, and the generated `workspace-hack`.

| Crate | Responsibility |
|---|---|
| `khora-core` | Traits and contracts: the lane contract (`Lane`, `LaneFamily`, `LaneInputs`/`LaneOutputs`, `Level`, `Cost`) and its plumbing (`LaneBus`, `OutputDeck`, `HostViews`, `Reserve`); `Agent`, `AgentRegistration` + `register_agent!` + `RegisteredAgent`, `Place`/`Layer`/`Wake`, `Activation`; the GORNA protocol types; `Residency`; backend traits; math; `Runtime`, `EngineContext`. |
| `khora-macros` | `#[derive(Component)]`, `#[derive(LaneInputs)]` / `#[derive(LaneOutputs)]`, `#[ergon_fn]`. |
| `khora-data` | CRPECS (`World`, storage, queries, change detection, AGDF layout), components, Flows, DataSystems, residencies, the frame graph and `register_pass!`, persistence (`scene/`). |
| `khora-control` | `DccService`, GORNA (rounds, the arbiter, the cost book), the frame governor, the `ExecutionScheduler` (lending, layers, waves, background pool), the Substrate dispatcher. |
| `khora-script` | Ergon: lexer, parser, checker, bytecode, VM, resume tiers. |
| `khora-telemetry` | `TelemetryService`, metrics, monitors. |
| `khora-infra` | Default backends: wgpu (`WgpuRenderSystem`, `WgpuDevice`, `WgpuPipelineSystem` + the `.wgsl` tree), winit, Rapier, CPAL + `DefaultMixBus`, Taffy, text (Parley + swash), monitors. |
| `khora-tool-ui` | The tools' design system (editor and hub); never reached by the SDK. |
| `khora-io` | `AssetService`, decoders, VFS, packs (signed), the trust boundary (`trust/`), `SerializationService`, `SceneLoadHub`, the project script compiler. |
| `khora-lanes` | One folder per family: its `LaneFamily`, its lanes, its `registration.rs`. |
| `khora-agents` | `LaneAgent` (`lane_agent/`) and the boot that builds one per registration (`registered_agent.rs`). |
| `khora-sdk` | The only public API for games: `EngineApp`, `GameWorld`, `Vessel`, `run_winit`/`run_default`, `prelude`. |
| `khora-editor`, `khora-hub`, `khora-runtime` | Apps on the SDK: the editor, the project launcher, the stamped player. |

## 3 — Trait map

| Trait | Defined in (`khora-core/src/` unless named) | Implemented by |
|---|---|---|
| `Lane` | `lane/contract/lane.rs` | every lane in khora-lanes |
| `LaneFamily` | `lane/contract/family.rs` | one per family folder (`AudioFamily`, `RenderFamily`, …) |
| `LaneInputs` / `LaneOutputs` | `lane/contract/io/` | `#[derive(LaneInputs)]` / `#[derive(LaneOutputs)]` |
| `PassSlot` | `lane/contract/io/pass.rs` | the pass slots, ordered by `register_pass!` |
| `Agent` | `agent/mod.rs` | `LaneAgent` only (xtask `only_lane_agent_implements_agent`) |
| `Residency` | `residency/mod.rs` | `MeshOnGpu`, `ImageOnGpu` (`khora-data/src/residency/`; the material residency, `gpu_material_sync`, registers there without the trait) |
| `RenderSystem`, `GraphicsDevice`, `PipelineSystem` | `renderer/traits/` | `WgpuRenderSystem`, `WgpuDevice`, `WgpuPipelineSystem` |
| `PhysicsProvider` | `physics/mod.rs` | `RapierPhysicsWorld` |
| `AudioDevice`, `AudioMixBus` | `audio/` | `CpalAudioDevice`, `DefaultMixBus` |
| `LayoutSystem` | `ui/layout.rs` | `TaffyLayoutSystem` |
| `Stored` | `asset/stored.rs` | every type an `Assets<T>` / `AssetHandle<T>` holds |
| `Asset` (kind), `LocalSource` | `asset/mod.rs`, `asset/local_source.rs` (`NoLocalSource`: `asset/no_local_source.rs`) | `Mesh`, `Box<dyn Material>`, `CpuTexture`, `Font`, `SoundData`, `PrefabAsset`; local sources `ProceduralMesh`, `InlineMaterial`, `NoLocalSource` |
| `SavedLocalSource<A>` | `khora-data/src/assets/saved_local_source.rs` | `ProceduralMesh`, `InlineMaterial`, `NoLocalSource` |
| `Component` | `khora-data/src/ecs/component.rs` | `#[derive(Component)]` |
| `Flow` | `khora-data/src/flow/mod.rs` | `RenderFlow`, `ShadowFlow`, `CameraFlow`, `PhysicsFlow`, `AudioFlow`, `UiFlow`, `ScriptFlow`, `ReservationFlow`, `PrefabCatalogFlow` |
| `AssetDecoder<A>` | `khora-io/src/asset/decoder.rs` | one per format in `khora-io/src/asset/decoders/` |

## 4 — Standard components

`Transform`, `GlobalTransform`, `Camera`, `Light`, `AssetRef<Mesh>`, `AssetRef<Box<dyn Material>>`, `RigidBody`, `Collider`,
`AudioSource`, `AudioListener`, `Parent`/`Children`, `Name`, `Tag`, `PrefabInstance` (instance root →
prefab link, `ToolAuthored`), `HandleComponent<T>`, and the UI set (`UiTransform`, `UiColor`, `UiText`,
`UiImage`, `UiBorder`). Each carries a `SemanticDomain` and a `ComponentProvenance`. A behavior is not a
Rust component: its script declares `Guard` (authored) and its state companion `Guard#state` (`Runtime` +
`resumable` — kept by game saves, never by scenes), registered while the engine runs.

## 5 — Critical file locations

| Area | Path |
|---|---|
| Lane contract / plumbing | `crates/khora-core/src/lane/` — `contract/` (`lane.rs`, `family.rs`, `level.rs`, `cost.rs`, `guard.rs`, `erased.rs`, `io/`), `plumbing/` (`bus.rs`, `deck.rs`, `host_views.rs`, `reserve.rs`, `staging.rs`, `lease.rs`) |
| Agent trait / registration / layers | `crates/khora-core/src/agent/` — `mod.rs`, `registration.rs` (`register_agent!`, `check_registrations`, `RegisteredAgent`), `background_checks.rs`, `contention.rs`, `activation.rs`, `layer/` (`Place`, `Layer`, `Wake`) |
| GORNA protocol types | `crates/khora-core/src/agent/gorna/` (`protocol.rs`, `offer.rs`, `report.rs`, `frame_report.rs`) |
| Families and lanes | `crates/khora-lanes/src/{audio,loading,physics,render,script,shadows,skybox,ui,overlay/{grid,wireframe,gizmo}}/` — `family.rs`, its lanes, `registration.rs` |
| `LaneAgent` / boot | `crates/khora-agents/src/lane_agent/` (`mod.rs`, `choice.rs`, `quotes.rs`, `layer.rs`, `wake.rs`), `crates/khora-agents/src/registered_agent.rs` |
| Residency | trait `crates/khora-core/src/residency/mod.rs`; `register_residency!` and the residencies `crates/khora-data/src/residency/` |
| ECS (CRPECS) / layout learner | `crates/khora-data/src/ecs/` (`world/`, `storage.rs`, `soa.rs`, `query/`, `layout/`) |
| Components | `crates/khora-data/src/ecs/components/` |
| Flows / DataSystems | `crates/khora-data/src/flow/`, `crates/khora-data/src/ecs/systems/` |
| Pass slots / frame graph | `crates/khora-data/src/render/frame_graph.rs` |
| Persistence | `crates/khora-data/src/scene/`, `crates/khora-core/src/ecs/persistent_id.rs`, `crates/khora-io/src/serialization.rs` |
| DCC / GORNA / arbiter / governor | `crates/khora-control/src/dcc_service/`, `gorna/` (`arbiter/`, `admission.rs`, `share.rs`, `cost_book.rs`), `governor/mod.rs` |
| Scheduler | `crates/khora-control/src/scheduler/` (`frame.rs`, `lending.rs`, `layers.rs`, `execution.rs`, `background.rs`), `worker_pool.rs` |
| Substrate dispatcher | `crates/khora-control/src/substrate/` |
| Shaders | `crates/khora-infra/src/graphics/shader/shaders/` (`pipelines/`, `lib/`), composed by `crates/khora-infra/src/graphics/wgpu/pipeline_system/mod.rs` |
| wgpu backend | `crates/khora-infra/src/graphics/wgpu/` (`system/`, `device/`) |
| Rapier / CPAL / Taffy | `crates/khora-infra/src/{physics/rapier,audio/cpal,ui/taffy}/` |
| Trust boundary / signed packs | `crates/khora-io/src/trust/`, `crates/khora-io/src/asset/signing.rs`, `crates/khora-sdk/src/game_seal.rs` |
| SDK entry / boot | `crates/khora-sdk/src/` (`lib.rs`, `game_world.rs`, `vessel.rs`, `traits.rs`, `engine/`) |
| Sandbox app | `examples/sandbox/src/` |

## 6 — Engine lifecycle

```
run_winit::<W, MyApp>(bootstrap)
  └─ window (hidden) → MyApp::new() → bootstrap(window, runtime, …)   ← the app installs its backends
  └─ engine services + DCC; Arc<Activation> knows every registered family
  └─ MyApp::setup(world, runtime) → MyApp::provide(runtime)
  └─ registered_agents() checked together (check_registrations), one LaneAgent each, every family on
Per frame: drain_inputs → run_app_update (activation.settle(), Boundary, app.update)
           → begin_render_frame → run_scheduler → end_render_frame → run_maintenance
```

The engine has no mode: families are switched with `Activation::{activate, deactivate}`, settled at the
next frame boundary; a DataSystem or Flow doing a family's work declares `Role::Serves(family)` and is
skipped while it is off. Slow startup work (project index, scene loads, script compiles) is filed by
ticket and done by the `loading` family off the frame. Detail: `docs/src/concepts/the-frame.md`.

## 7 — Agents and families

```rust
pub trait Agent: Send + Sync {
    fn id(&self) -> AgentId;
    fn offer(&mut self, request: &OfferRequest) -> Offer;                 // every row, priced
    fn apply(&mut self, epoch: Epoch, envelope: Option<&Envelope>,
             pin: Option<StrategyId>, allowed: Option<&[StrategyId]>);    // choose the row that fits
    fn withdraw(&mut self) {}                                             // a family switched back on
    fn report(&self) -> Report;                                           // row, measured, done, health, stalled
    fn place(&self) -> Arc<Mutex<Place>>;                                 // where its layer runs
    fn wake(&mut self, why: &[Wake], place: &mut Place, ctx: &mut EngineContext<'_>) {}
    fn execution_timing(&self) -> ExecutionTiming { .. }                  // one phase, default OUTPUT
    fn contention(&self) -> Contention { .. }                            // derived from the lanes' I/O
}
```

No `execute`: the scheduler runs each family's installed layer at its place every frame and wakes the
agent only for an event. A fixed-step family's step lives at its place (`Place::fixed_step`). The only
implementor is `LaneAgent`, built from a family's registration.

| Family (`AgentId`) | Phase | Importance | Lanes / levels |
|---|---|---|---|
| `renderer` | OUTPUT | Critical | `SimpleUnlitLane` "unlit" (LowPower) / `LitForwardLane` "lit forward" (Balanced, default) / `ForwardPlusLane` "forward+" (HighPerformance) |
| `shadows` | OBSERVE | Important | `ShadowsLane`: "low-res shadows" (LowPower) / "medium shadows" (Balanced) / "standard shadows" (HighPerformance) |
| `grid`, `wireframe`, `gizmo` | OUTPUT, after `renderer` | Optional | `GridLane`, `WireframeLane`, `GizmoLane` |
| `skybox` | OUTPUT, after `renderer` | Important | `SkyboxLane` |
| `physics` | TRANSFORM | Critical | `StandardPhysicsLane`, one level, `Exact`; fixed step |
| `ui` | OUTPUT | Important | `UiLane` |
| `audio` | TRANSFORM | Important | `SpatialMixingLane`: 8 / 32 / 128 voices |
| `script` | TRANSFORM | Critical | `BudgetedScriptLane`: "critical" (LowPower) / "whole" (Balanced, default), `Exact` |
| `loading` | BACKGROUND | Optional | `RushLane` "rush" (HighPerformance, default) / `PacedLane` "paced" (Balanced), `Exact` |

---

*End of architecture brief.*
