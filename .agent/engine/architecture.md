# Khora Engine — Architecture Brief (engine profile)

Dense reference for AI agents. Full narrative in [`../../docs/src/concepts/clad.md`](../../docs/src/concepts/clad.md).

- Document — Khora Architecture Brief v2.0
- Status — Active

---

## 0 — Use codegraph first

A **codegraph** MCP server indexes every symbol, edge, and file in the workspace (sub-millisecond
reads, ~1s lag behind writes). **Query it before grepping or reading whole files:**

- "what is X?" → `codegraph_search`; "the deal with this area?" → `codegraph_context`.
- "how does X reach Y?" → `codegraph_trace`; "what calls / what does it call?" → `codegraph_callers` / `codegraph_callees`.
- "show several related symbols" → `codegraph_explore`. Use Read/Grep only to confirm a specific detail codegraph didn't cover.

## 1 — CLAD layers

```
SAA (the why)                       CLAD (the how)
─────────────────────────           ──────────────────────────
Dynamic Context Core (DCC)          khora-control
GORNA Protocol                      khora-control
Intelligent Subsystem Agents        khora-agents
Adaptive Game Data Flows (AGDF)     khora-data (CRPECS + Flows)
Semantic Interfaces & Contracts     khora-core
I/O Services                        khora-io
Observability & Telemetry           khora-telemetry
Hardware & OS Interaction           khora-infra
```

### CLAD descent and Substrate Pass

CLAD is the **command path** of a frame:

```
Control ──► Agent ──► Lane ──► Data (via typed Views)
       budget   selects   reads bus / writes deck
```

Around it the Scheduler runs a **Substrate Pass** that executes the Data layer's self-maintenance —
invariants (`DataSystem`s) and presentation (`Flow`s) — and provides Lanes with their typed input
(`LaneBus`, `khora-core/src/lane/bus.rs`) and output (`OutputDeck`, `khora-core/src/lane/deck.rs`).
The Substrate Pass is *not* a descent: it is Data maintaining and projecting itself, called by the
Scheduler because the Scheduler owns the tick. Implemented in `khora-control/src/substrate/`.

**Only Agents compete for the frame budget** (the descent's auction). The Data layer does not bid.
AGDF layout adaptation is a self-optimization internal to `khora-data`, decided locally from measured
access patterns and self-bounded by a cost/benefit test; the DCC only **observes** it via telemetry.

```
Pass A — Substrate (Scheduler):   DataSystem invariants → Flows publish Views into LaneBus
Pass B — CLAD descent (Agents):   choose lane by budget → fill a LaneContext (type map) → Lane.execute(&mut ctx)
Pass C — I/O boundary (Engine):   drain OutputDeck for submit/present
```

### AGDF realisation

**AGDF** is the online adaptation of data **layout** to hardware and observed access patterns — a
*representation* (HOW) adaptation, never *semantic* (WHAT). It lives in CRPECS (a layout policy per
component column, access instrumentation, a self-bounded repack step), default layout plain SoA, so it
is additive and inert until a column is profiled as worth re-tiling (e.g. SoA → AoSoA for SIMD). Flows
are **read-only projectors** (`select → project`); they never mutate the World. A Flow may opt into
**view caching** via `Flow::cache_key`: the `register_flow!` trampoline republishes the previous View
(cheap clone) when the key — `World::instance_id` + per-domain change epochs (`World::domain_epoch`,
the tick of the domain's last write or row move, O(1); handing out access raises nothing) + any
runtime-state fingerprint — is unchanged. Finer change detection (row ticks, `Changed<T>`, `Mut<T>`):
`docs/src/concepts/change-detection.md`. Cached today:
Audio/Render/Shadow flows; Ui/Physics stay uncached (no change signal / mutates every frame).
Representation-only: a cached View is bit-identical to a re-projected one. Gameplay-relevance
gating (e.g. detach `RigidBody` by distance) is **not** AGDF — it changes the simulation, so it is an
opt-in, developer-authored `DataSystem`. See `khora-data/src/ecs/layout/` (`LayoutAdvisor`; bandits `Ucb1` in `bandit.rs`).

## 2 — Crate dependency graph

```
khora-sdk
├── khora-agents
│   ├── khora-lanes (← khora-core, khora-data, khora-io)
│   ├── khora-infra
│   ├── khora-io  ├── khora-telemetry
│   ├── khora-core └── khora-data
├── khora-control (← khora-core, khora-data)
├── khora-infra
├── khora-telemetry (← khora-core)
└── khora-core (← khora-macros)
```

Dependencies flow downward only. `khora-core` is the foundation. `khora-editor`, `khora-runtime`,
`khora-hub`, and `sandbox` sit on top of `khora-sdk`.

## 3 — Crate responsibilities (16 workspace members)

| Crate | Layer | Responsibility |
|---|---|---|
| `khora-core` | Foundation | Traits (`Lane`, `Agent`, `RenderSystem`, `PhysicsProvider`, `AudioDevice`, `LayoutSystem`, `Asset`, VFS), math (`Vec2/3/4`, `Mat3/4`, `Quaternion`, `Aabb`, `LinearRgba`, `simd`), GORNA types, error hierarchy, `ServiceRegistry`, `EngineContext`, the memory counters. |
| `khora-macros` | Foundation | `#[derive(Component)]` proc macro. Path crate (not a workspace member). |
| `khora-data` | Data | CRPECS ECS (`World`, storage, query, `SemanticDomain`, `ComponentProvenance`), SoA/AGDF layout (`LayoutAdvisor`, `Ucb1`), `Assets<T>`, UI components, Flows, `EcsMaintenance`, and **persistence** (`scene/`: `SceneRecord`, encodings text/compact/msgpack, schema-bound snapshot, prefab links, game `SaveRecord`, atomic `apply`, `LoadReport`). |
| `khora-control` | Control | `DccService` (agent lifecycle), `GornaArbitrator` (budget fitting + replay), cost model, frame governor, Substrate dispatcher, situational `Context` (thermal/battery/phase). |
| `khora-telemetry` | Infra | `TelemetryService`, `MetricsRegistry`, `MonitorRegistry`, telemetry event storage. |
| `khora-lanes` | Lanes | Render (Unlit, LitForward, Forward+, StandardPbr, Shadow, Overlay: Grid/Wireframe/Gizmo, Skybox, UI), Physics (Standard, CCD), Audio (SpatialMixing, SourceUpdate), UI (StandardUi, TaffyLayout), Script (Budgeted). |
| `khora-infra` | Infra | Default backends: `WgpuRenderSystem`/`WgpuDevice`, `WinitWindow` + input, Rapier3D physics, CPAL audio, Taffy layout, GPU/Memory/Vram monitors, `SaaTrackingAllocator`, `DefaultMixBus`. Each implements a `khora-core` trait and is swappable. |
| `khora-io` | Data | `AssetService`, `SerializationService` (scene save/load, game `save_game`/`load_game`, `AssetPrefabs`), VFS, `AssetIo`, `PackLoader`/`FileLoader`, decoders (glTF, OBJ, Symphonia audio, texture, font). |
| `khora-agents` | Agents | `RenderAgent`, `ShadowAgent`, `OverlayAgent`, `SkyboxAgent`, `PhysicsAgent`, `UiAgent`, `AudioAgent`, `ScriptAgent` + `PhysicsQueryService`. |
| `khora-sdk` | Public API | `EngineCore` + `run_winit` entry, `GameWorld` (safe ECS façade), `EngineApp`/`AgentProvider`/`PhaseProvider` traits, `WindowConfig`, `Vessel` + `spawn_plane`/`spawn_cube_at`/`spawn_sphere`, `prelude`. **The only crate game devs import.** |
| `khora-editor` | Application | Editor app on the SDK — panels, gizmos, dock, hot-reload, command palette. |
| `khora-runtime` | Application | Generic player binary stamped with packed assets; boots via `khora_sdk::run_default`. |
| `sandbox` | Example | Example game using only the SDK (`examples/sandbox`). |
| `xtask` | Tool | Build automation — `cargo xtask {build,test,check,format,clippy,all,…}`. |
| `khora-hub` | Tool | Project manager / engine launcher; reaches egui only via `khora_sdk::tool_ui`. |

## 4 — Trait map

| Trait | Defined in | Implemented by |
|---|---|---|
| `Lane` | khora-core (`lane/`) | All lane types in khora-lanes |
| `Agent` | khora-core (`agent/`) | All agent types in khora-agents |
| `RenderSystem` | khora-core | `WgpuRenderSystem` in khora-infra |
| `PhysicsProvider` | khora-core | `RapierPhysicsWorld` in khora-infra |
| `AudioDevice` | khora-core | `CpalAudioDevice` in khora-infra |
| `LayoutSystem` | khora-core | `TaffyLayoutSystem` in khora-infra |
| `Asset` | khora-core | All loadable asset types |
| `Component` | khora-data (derive) | All ECS components |
| `AssetDecoder<A>` | khora-io | Per-format decoders |
| `Flow` | khora-data | `RenderFlow`, `PhysicsFlow`, `ShadowFlow`, `AudioFlow`, `UiFlow` |

## 5 — Standard components

`Transform`, `GlobalTransform`, `Camera`, `Light`, `MeshRef`, `MaterialRef`, `RigidBody`, `Collider`,
`AudioSource`, `AudioListener`, `Parent`/`Children`, `Name`, `Tag`, `PrefabInstance` (instance root →
prefab link, `ToolAuthored`), `HandleComponent<T>`, and the UI set (`UiTransform`, `UiColor`, `UiText`,
`UiImage`, `UiBorder`). Each carries a `SemanticDomain` and a `ComponentProvenance`. A behavior is not a
Rust component: its script declares `Guard` (authored) and its state companion `Guard#state` (`Runtime` +
`resumable` — kept by game saves, never by scenes), registered while the engine runs.

## 6 — Critical file locations

| Area | Path |
|---|---|
| Lane trait / bus / deck | `crates/khora-core/src/lane/` (`mod.rs`, `bus.rs`, `deck.rs`) |
| Agent trait / `AgentId` | `crates/khora-core/src/agent/`, GORNA types `crates/khora-core/src/agent/gorna/mod.rs` |
| Math / SIMD | `crates/khora-core/src/math/` (`simd.rs`) |
| ECS (CRPECS) / layout learner | `crates/khora-data/src/ecs/` (`world/`, `storage.rs`, `soa.rs`, `layout/`) |
| Components / registrations | `crates/khora-data/src/ecs/components/` |
| Flows / DataSystems | `crates/khora-data/src/flow/`, `crates/khora-data/src/ecs/systems/` |
| Persistence | `crates/khora-data/src/scene/` (`record/`, `encoding/`, `snapshot/`, `prefab/`, `save.rs`, `apply.rs`, `capture.rs`, `file.rs`, `retired.rs`), `crates/khora-core/src/ecs/persistent_id.rs`, `crates/khora-io/src/serialization.rs` |
| DCC / GORNA / cost / governor | `crates/khora-control/src/dcc_service/`, `gorna/`, `cost_model.rs`; frame governor `crates/khora-control/src/governor/mod.rs` |
| Substrate dispatcher | `crates/khora-control/src/substrate/` |
| Render lanes / shaders | `crates/khora-lanes/src/render_lane/`; shaders in `crates/khora-infra/src/graphics/shader/shaders/` (`pipelines/`, `lib/`), composed by `graphics/wgpu/pipeline_system/mod.rs` |
| Shadow / overlay / skybox / ui / physics / audio / script lanes | `crates/khora-lanes/src/{shadow_lane,overlay_lane,skybox_lane,ui_lane,physics_lane,audio_lane,script_lane}/` — one folder per agent |
| wgpu backend | `crates/khora-infra/src/graphics/wgpu/` (`system/`, `device/`) |
| Rapier / CPAL / Taffy | `crates/khora-infra/src/{physics/rapier,audio/cpal,ui/taffy}/` |
| Agents | `crates/khora-agents/src/{render,shadow,overlay,skybox,physics,ui,audio,script}_agent/` |
| SDK entry / GameWorld / Vessel | `crates/khora-sdk/src/lib.rs`, `game_world.rs`, `vessel.rs` |
| Sandbox app | `examples/sandbox/src/` (`game.rs`, `player.rs`, `assets.rs`) |

## 7 — Engine lifecycle

```
run_winit::<W, MyApp>(bootstrap)    ← entry point
  └─ MyApp::window_config()         ← read WindowConfig
  └─ window opened
  └─ bootstrap(window, runtime, _)  ← user registers backends into runtime.backends/resources
  └─ MyApp::new()                   ← simple constructor, no context
  └─ engine init                    ← default services, DCC + agents registered,
                                       Arc<Activation> published (every family on); app agents
                                       admitted after register_agents; Role::Serves checked
  └─ GameWorld::set_prefabs(…)      ← when an AssetService is present (AssetPrefabs)
  └─ MyApp::setup(world, runtime)   ← cache services, spawn entities
Per frame:
  app.before_frame(…)               ← the editor switches families here (transport)
  activation.settle()               ← frame boundary: one state for the whole frame; Drop
                                       mailboxes of families off emptied; ActivationChanged → DCC
  app.update(world, inputs)         ← user game logic
  world maintenance (DataSystems)   ← ECS GC, transform propagation, provider sync
  scheduler.run_frame()             ← Substrate Pass + the active families' agents
```

**Activation (no engine mode).** The engine has no mode; families are switched on and off.
`khora_core::agent::Activation` (`khora-core/src/agent/activation.rs`) is published as `Arc<Activation>`
at bootstrap (`khora-sdk/src/engine/bootstrap.rs`), knowing every registered family (lane families, the
script agent, the app's agents). Every family starts on; `activate`/`deactivate` are requests, an unknown
family is refused (`UnknownFamily`). `run_app_update` (`khora-sdk/src/engine/frame.rs`) opens with
`settle()`, after `before_frame` and before the first DataSystem phase, so every phase, Flow, the
scheduler and the DCC see one state; a later request counts from the next frame. On a change the SDK
sends `TelemetryEvent::ActivationChanged(inactive)`: the DCC keeps it in `Context.inactive`, clears its
run reports, and opens an epoch (`AgentsChanged`); only active families are asked to offer and are lent.
DataSystems and Flows declare a `Role` (`khora-data/src/ecs/role.rs`): `Upkeep` runs every frame,
`Serves(φ)` is skipped while φ is off and resumes with `SystemContext.resumed` /
`FlowContext { resumed }`. A family may be **scoped** (`scope(family, marker_name)`): it counts as on,
the substrate resolves the marker (`role_scope`, `khora-control/src/substrate/data_system_runner.rs`) and
hands `SystemContext.scope` / `FlowContext.scope`; Flows project what `Selection::admits`
(`khora-data/src/flow/selection.rs`), cached views are keyed by the scope and who it admits, and
`ScriptView.left_scope` makes the lane forget without a farewell. Scripts and audio honour it; physics
does not yet. A family's channels declare a `Mailbox` (`khora-core/src/event/mailbox.rs`):
`BetweenSessions::Drop` mail is emptied at each boundary while the family is off, `Hold` is kept. Family
ids: `khora_core::{script::SCRIPT, physics::PHYSICS, audio::AUDIO}`. The editor's transport
(`khora-editor/src/commands/transport.rs`, `process_transport` in `before_frame`) switches SCRIPT, PHYSICS
and AUDIO off while editing (scale 1) and on at Play; Pause only sets the scale to 0.

App implements `EngineApp + AgentProvider + PhaseProvider` (composite SDK trait). Detail in
[`../../docs/src/concepts/the-frame.md`](../../docs/src/concepts/the-frame.md).

## 8 — Agents (one per CLAD domain)

```rust
pub trait Agent: Send + Sync {
    fn id(&self) -> AgentId;
    fn negotiate(&mut self, request: NegotiationRequest) -> NegotiationResponse;
    fn apply_budget(&mut self, budget: ResourceBudget);
    fn report_status(&self) -> AgentStatus;
    fn on_initialize(&mut self, ctx: &mut EngineContext<'_>) {}  // once after registration
    fn execute(&mut self, ctx: &mut EngineContext<'_>);          // every frame
    fn as_any(&self) -> &dyn Any;  fn as_any_mut(&mut self) -> &mut dyn Any;
}
```

| Agent | `LaneKind` | Strategies |
|---|---|---|
| `RenderAgent` | Render | Unlit / LitForward / Forward+ / StandardPbr |
| `ShadowAgent` | Shadow | Standard (2048² + 512² cube, ≈88 MiB) / Medium (1024² + 256², ≈22 MiB) / LowRes (512² + 128², ≈5.5 MiB) — HighPerformance/Balanced/LowPower |
| `OverlayAgent` | Render | parallel post-render lanes: Grid → Wireframe → Gizmo |
| `SkyboxAgent` | Render | environment background (`Skybox` lane) |
| `PhysicsAgent` | Physics | Standard / Simplified |
| `UiAgent` | Ui | Layout + Render |
| `AudioAgent` | Audio | source count / quality |
| `ScriptAgent` | Script | `Budgeted` lane; LowPower / Balanced / HighPerformance fuel — family `SCRIPT`, off while the editor edits |

WGSL composition: the `.wgsl` files live in `khora-infra/src/graphics/shader/shaders/`
(`pipelines/` entry points, `lib/` reusable modules) and are embedded with `include_str!`.
A render lane never handles source — it **names** a pipeline in its spec
(`shader: "khora::pipelines::forward_plus"`), and the `PipelineSystem` backend
(`khora-infra/src/graphics/wgpu/pipeline_system/mod.rs`) resolves the `#import` graph via `naga_oil`,
compiles, and caches. There is no `ShaderRegistry` type; earlier revisions of these docs
named one.

---

*End of architecture brief.*
