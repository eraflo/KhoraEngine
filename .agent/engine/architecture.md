# Khora Engine — Architecture Brief (engine profile)

Dense reference for AI agents. Full narrative in [`../../docs/src/02_architecture.md`](../../docs/src/02_architecture.md).

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
Pass B — CLAD descent (Agents):   choose lane by budget → Lane.execute(LaneContext{bus, deck, budget})
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
bumped O(1) at every semantic mutation) + any runtime-state fingerprint — is unchanged. Cached today:
Audio/Render/Shadow flows; Ui/Physics stay uncached (no change signal / mutates every frame).
Representation-only: a cached View is bit-identical to a re-projected one. Gameplay-relevance
gating (e.g. detach `RigidBody` by distance) is **not** AGDF — it changes the simulation, so it is an
opt-in, developer-authored `DataSystem`. See `khora-data/src/ecs/layout.rs` (`LayoutAdvisor`, `Ucb1`).

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
`hub`, and `sandbox` sit on top of `khora-sdk`.

## 3 — Crate responsibilities (16 workspace crates)

| Crate | Layer | Responsibility |
|---|---|---|
| `khora-core` | Foundation | Traits (`Lane`, `Agent`, `RenderSystem`, `PhysicsProvider`, `AudioDevice`, `LayoutSystem`, `Asset`, VFS), math (`Vec2/3/4`, `Mat3/4`, `Quaternion`, `Aabb`, `LinearRgba`, `simd`), GORNA types, error hierarchy, `ServiceRegistry`, `EngineContext`, `SaaTrackingAllocator`. |
| `khora-macros` | Foundation | `#[derive(Component)]` proc macro. Path crate (not a workspace member). |
| `khora-data` | Data | CRPECS ECS (`World`, storage, query, `SemanticDomain`), SoA/AGDF layout (`LayoutAdvisor`, `Ucb1`), `Assets<T>`, UI components, scene definitions, Flows, `EcsMaintenance`. |
| `khora-control` | Control | `DccService` (agent lifecycle), `GornaArbitrator` (budget fitting + replay), cost model, PID budget, Substrate dispatcher, situational `Context` (thermal/battery/phase). |
| `khora-telemetry` | Infra | `TelemetryService`, `MetricsRegistry`, `MonitorRegistry`, telemetry event storage. |
| `khora-lanes` | Lanes | Render (Unlit, LitForward, Forward+, StandardPbr, Shadow, Overlay: Grid/Emissive/Wireframe/Gizmo, UI), Physics (Standard, CCD), Audio (SpatialMixing, SourceUpdate), UI (StandardUi, TaffyLayout), Script (Budgeted). |
| `khora-infra` | Infra | Default backends: `WgpuRenderSystem`/`WgpuDevice`, `WinitWindow` + input, Rapier3D physics, CPAL audio, Taffy layout, GPU/Memory/Vram monitors. Each implements a `khora-core` trait and is swappable. |
| `khora-io` | Data | `AssetService`, `SerializationService`, VFS, `AssetIo`, `PackLoader`/`FileLoader`, decoders (glTF, OBJ, Symphonia audio, texture, font). |
| `khora-agents` | Agents | `RenderAgent`, `ShadowAgent`, `OverlayAgent`, `PhysicsAgent`, `UiAgent`, `AudioAgent` + `PhysicsQueryService`. |
| `khora-plugins` | Extension | Plugin loading and registration. |
| `khora-sdk` | Public API | `EngineCore` + `run_winit` entry, `GameWorld` (safe ECS façade), `EngineApp`/`AgentProvider`/`PhaseProvider` traits, `WindowConfig`, `Vessel` + `spawn_plane`/`spawn_cube_at`/`spawn_sphere`, `prelude`. **The only crate game devs import.** |
| `khora-editor` | Application | Editor app on the SDK — panels, gizmos, dock, hot-reload, command palette. |
| `khora-runtime` | Application | Generic player binary stamped with packed assets; boots via `khora_sdk::run_default`. |
| `sandbox` | Example | Example game using only the SDK (`examples/sandbox`). |
| `xtask` | Tool | Build automation — `cargo xtask {build,test,check,format,clippy,all,…}`. |
| `hub` | Tool | Project manager / engine launcher; reaches egui only via `khora_sdk::tool_ui`. |

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

`Transform`, `GlobalTransform`, `Camera`, `Light`, `MaterialComponent`, `RigidBody`, `Collider`,
`AudioSource`, `AudioListener`, `Parent`/`Children`, `Name`, `Tag`, `HandleComponent<T>`, and the UI
set (`UiTransform`, `UiColor`, `UiText`, `UiImage`, `UiBorder`). Each carries a `SemanticDomain`.

## 6 — Critical file locations

| Area | Path |
|---|---|
| Lane trait / bus / deck | `crates/khora-core/src/lane/` (`mod.rs`, `bus.rs`, `deck.rs`) |
| Agent trait / `AgentId` | `crates/khora-core/src/agent/`, GORNA types `crates/khora-core/src/control/gorna/` |
| Math / SIMD | `crates/khora-core/src/math/` (`simd.rs`) |
| ECS (CRPECS) / layout learner | `crates/khora-data/src/ecs/` (`world.rs`, `storage.rs`, `soa.rs`, `layout.rs`) |
| Components / registrations | `crates/khora-data/src/ecs/components/` |
| Flows / DataSystems | `crates/khora-data/src/flow/`, `crates/khora-data/src/ecs/systems/` |
| DCC / GORNA / cost / PID | `crates/khora-control/src/service.rs`, `gorna/mod.rs`, `cost_model.rs`; PID `crates/khora-core/src/control/pid.rs` |
| Substrate dispatcher | `crates/khora-control/src/substrate/` |
| Render lanes / shaders | `crates/khora-lanes/src/render_lane/` (`shaders/pipelines/`, `shaders/lib/`, `shader_registry.rs`) |
| Physics / audio / ui lanes | `crates/khora-lanes/src/{physics_lane,audio_lane,ui_lane}/` |
| wgpu backend | `crates/khora-infra/src/graphics/wgpu/` (`system.rs`, `device.rs`) |
| Rapier / CPAL / Taffy | `crates/khora-infra/src/{physics/rapier,audio/backends/cpal,ui/taffy}/` |
| Agents | `crates/khora-agents/src/{render,shadow,overlay,physics,ui,audio}_agent/` |
| SDK entry / GameWorld / Vessel | `crates/khora-sdk/src/lib.rs`, `game_world.rs`, `vessel.rs` |
| Sandbox app | `examples/sandbox/src/main.rs` |

## 7 — Engine lifecycle

```
run_winit::<W, MyApp>(bootstrap)    ← entry point
  └─ MyApp::window_config()         ← read WindowConfig
  └─ window opened
  └─ bootstrap(window, runtime, _)  ← user registers backends into runtime.backends/resources
  └─ MyApp::new()                   ← simple constructor, no context
  └─ engine init                    ← default services + DCC + agents registered
  └─ MyApp::setup(world, runtime)   ← cache services, spawn entities
Per frame:
  app.update(world, inputs)         ← user game logic
  world maintenance (DataSystems)   ← ECS GC, transform propagation, provider sync
  scheduler.run_frame()             ← Substrate Pass + agents dispatch lanes
```

App implements `EngineApp + AgentProvider + PhaseProvider` (composite SDK trait). Detail in
[`../../docs/src/03_lifecycle.md`](../../docs/src/03_lifecycle.md).

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
| `OverlayAgent` | Render | parallel post-render lanes: Grid → Emissive → Wireframe → Gizmo |
| `PhysicsAgent` | Physics | Standard / Simplified |
| `UiAgent` | Ui | Layout + Render (editor mode) |
| `AudioAgent` | Audio | source count / quality |

WGSL composition: the `.wgsl` files live in `khora-infra/src/graphics/shader/shaders/`
(`pipelines/` entry points, `lib/` reusable modules) and are embedded with `include_str!`.
A render lane never handles source — it **names** a pipeline in its spec
(`shader: "khora::pipelines::forward_plus"`), and the `PipelineSystem` backend
(`khora-infra/src/graphics/shader/system.rs`) resolves the `#import` graph via `naga_oil`,
compiles, and caches. There is no `ShaderRegistry` type; earlier revisions of these docs
named one.

---

*End of architecture brief.*
