# Khora Engine — Architecture Brief

Dense reference for AI agents. For the full narrative, see [`docs/src/02_architecture.md`](../docs/src/02_architecture.md).

- Document — Khora Architecture Brief v1.0
- Status — Active
- Date — May 2026

---

## Contents

1. CLAD layers
2. Crate dependency graph
3. Crate responsibilities
4. Trait map
5. Standard components
6. Critical file locations
7. Engine lifecycle
8. Agent lifecycle

---

## 01 — CLAD layers

```
SAA (the why)                       CLAD (the how)
─────────────────────────           ──────────────────────────
Dynamic Context Core (DCC)          khora-control
GORNA Protocol                      khora-control
Intelligent Subsystem Agents        khora-agents
Adaptive Game Data Flows            khora-data
Semantic Interfaces & Contracts     khora-core
I/O Services                        khora-io
Observability & Telemetry           khora-telemetry
Hardware & OS Interaction           khora-infra
```

The split is the architecture. Read the full mapping in [`docs/src/02_architecture.md`](../docs/src/02_architecture.md).

## 02 — Crate dependency graph

```
khora-sdk
├── khora-agents
│   ├── khora-lanes
│   │   ├── khora-core
│   │   └── khora-data
│   ├── khora-infra
│   ├── khora-core
│   └── khora-data
├── khora-control
│   ├── khora-core
│   └── khora-data
├── khora-infra
├── khora-telemetry
│   └── khora-core
└── khora-core
```

Dependencies flow downward only. `khora-core` is the foundation — it depends on nothing else in the workspace.

## 03 — Crate responsibilities

| Crate | Layer | Responsibility |
|---|---|---|
| `khora-core` | Foundation | Traits (Lane, Agent, RenderSystem, PhysicsProvider, AudioDevice, LayoutSystem, Asset, VFS), math (`Vec2/3/4`, `Mat3/4`, `Quat`, `Aabb`, `LinearRgba`), GORNA types, error hierarchy, ServiceRegistry, EngineContext, SaaTrackingAllocator |
| `khora-macros` | Foundation | `#[derive(Component)]` proc macro |
| `khora-data` | Data | CRPECS ECS (World, Archetype, Query, Page, SemanticDomain), `Assets<T>` storage, UI components, scene definitions, `EcsMaintenance` |
| `khora-control` | Control | DccService (agent lifecycle), GornaArbitrator (budget fitting), HeuristicEngine (death spiral detection), Context (thermal/battery/execution phase) |
| `khora-telemetry` | Infra | TelemetryService, MetricsRegistry, MonitorRegistry, telemetry event storage |
| `khora-lanes` | Lanes | Render (Unlit, LitForward, Forward+, Shadow, UI, Extract), Physics (Standard, Debug), Audio (SpatialMixing), Asset loaders (glTF, OBJ, WAV, Symphonia, Texture, Font, Pack), ECS (Compaction), Scene (Definition, Recipe, Archetype serialization, TransformPropagation), UI (StandardUi) |
| `khora-infra` | Infra | Current default backends — WgpuRenderSystem and WgpuDevice (GPU), WinitWindow, input translation, Rapier3D physics, CPAL audio, Taffy layout, GpuMonitor / MemoryMonitor / VramMonitor. Each backend implements a `khora-core` trait and is swappable. |
| `khora-io` | Data | AssetService, SerializationService, VFS, AssetIo, PackLoader, FileLoader |
| `khora-agents` | Agents | RenderAgent, ShadowAgent, PhysicsAgent, UiAgent, AudioAgent + PhysicsQueryService |
| `khora-plugins` | Extension | Plugin loading and registration |
| `khora-sdk` | Public API | EngineCore + `run_winit` entry point, GameWorld (safe ECS facade), EngineApp / AgentProvider / PhaseProvider traits, WindowConfig, Vessel builder + spawn_plane / spawn_cube_at / spawn_sphere helpers |
| `khora-editor` | Application | Editor application built on khora-sdk |

## 04 — Trait map

| Trait | Defined in | Implemented by |
|---|---|---|
| `Lane` | khora-core | All lane types in khora-lanes |
| `Agent` | khora-core | All agent types in khora-agents |
| `RenderSystem` | khora-core | `WgpuRenderSystem` in khora-infra |
| `PhysicsProvider` | khora-core | Rapier3D backend in khora-infra |
| `AudioDevice` | khora-core | CPAL backend in khora-infra |
| `LayoutSystem` | khora-core | `TaffyLayoutSystem` in khora-infra |
| `Asset` | khora-core | All loadable asset types |
| `Component` | khora-data | All ECS components (via derive macro) |
| `AssetDecoder<A>` | khora-lanes | Per-format decoder lanes |

## 05 — Standard components

| Component | Domain | Purpose |
|---|---|---|
| `Transform` | All | Local position / rotation / scale |
| `GlobalTransform` | All | World-space computed transform |
| `Camera` | Render | Projection + view configuration |
| `Light` | Render | Light type, color, intensity, shadow config |
| `MaterialComponent` | Render | Material reference (handle) |
| `RigidBody` | Physics | Body type, mass, velocity, CCD |
| `Collider` | Physics | Shape descriptor for collision |
| `AudioSource` | Audio | Audio clip, volume, spatial flags |
| `AudioListener` | Audio | Listener position for 3D audio |
| `Parent` / `Children` | Scene | Entity hierarchy |
| `HandleComponent` | Asset | Generic asset handle wrapper |
| `UiTransform` | UI | Position, size, anchoring |
| `UiColor` | UI | Background color |
| `UiText` | UI | Text content, font, color |
| `UiImage` | UI | Texture handle, scale mode |
| `UiBorder` | UI | Border width, color |

## 06 — Critical file locations

### khora-core
| Area | Path |
|---|---|
| Lane trait | `crates/khora-core/src/lane/` |
| Agent trait | `crates/khora-core/src/agent/` |
| Math | `crates/khora-core/src/math/` |
| Physics trait | `crates/khora-core/src/physics/` |
| Audio trait | `crates/khora-core/src/audio/` |
| Asset trait + VFS | `crates/khora-core/src/asset/`, `crates/khora-core/src/vfs/` |
| UI layout trait | `crates/khora-core/src/ui/` |
| Scene & Serialization | `crates/khora-core/src/scene/` |
| GORNA types | `crates/khora-core/src/control/gorna/` |
| ServiceRegistry | `crates/khora-core/src/service_registry.rs` |
| EngineContext | `crates/khora-core/src/context.rs` |
| Error hierarchy | `crates/khora-core/src/renderer/error.rs` |

### khora-data
| Area | Path |
|---|---|
| ECS (CRPECS) | `crates/khora-data/src/ecs/` |
| Components | `crates/khora-data/src/ecs/components/` |
| UI components | `crates/khora-data/src/ui/` |
| Asset storage | `crates/khora-data/src/assets/` |
| Tracking allocator | `crates/khora-data/src/allocators/` |

### khora-control
| Area | Path |
|---|---|
| DCC service | `crates/khora-control/src/service.rs` |
| GORNA arbitrator | `crates/khora-control/src/gorna/` |
| Heuristics | `crates/khora-control/src/analysis.rs` |

### khora-lanes
| Area | Path |
|---|---|
| Render lanes | `crates/khora-lanes/src/render_lane/` |
| Physics lanes | `crates/khora-lanes/src/physics_lane/` |
| Audio lanes | `crates/khora-lanes/src/audio_lane/` |
| Asset loaders | `crates/khora-lanes/src/asset_lane/loading/` |
| Scene lanes | `crates/khora-lanes/src/scene_lane/` |
| UI lane | `crates/khora-lanes/src/ui_lane/` |
| ECS lane | `crates/khora-lanes/src/ecs_lane/` |
| WGSL shaders | `crates/khora-lanes/src/render_lane/shaders/` |

### khora-infra
| Area | Path |
|---|---|
| Render system | `crates/khora-infra/src/graphics/wgpu/system.rs` |
| GPU device | `crates/khora-infra/src/graphics/wgpu/device.rs` |
| Window (winit) | `crates/khora-infra/src/platform/window/` |
| Input | `crates/khora-infra/src/platform/input.rs` |
| Physics backend | `crates/khora-infra/src/physics/` |
| Audio backend | `crates/khora-infra/src/audio/` |
| Taffy layout | `crates/khora-infra/src/ui/taffy/` |
| Resource monitors | `crates/khora-infra/src/telemetry/` |

### khora-agents
| Area | Path |
|---|---|
| RenderAgent | `crates/khora-agents/src/render_agent/` |
| ShadowAgent | `crates/khora-agents/src/shadow_agent/` |
| UiAgent | `crates/khora-agents/src/ui_agent/` |
| PhysicsAgent | `crates/khora-agents/src/physics_agent/` |
| AudioAgent | `crates/khora-agents/src/audio_agent/` |

### khora-sdk and others
| Area | Path |
|---|---|
| SDK entry | `crates/khora-sdk/src/lib.rs` |
| GameWorld | `crates/khora-sdk/src/game_world.rs` |
| Vessel primitives | `crates/khora-sdk/src/vessel.rs` |
| Telemetry service | `crates/khora-telemetry/src/service.rs` |
| Component macro | `crates/khora-macros/src/` |
| Sandbox app | `examples/sandbox/src/main.rs` |

## 07 — Engine lifecycle

```
run_winit::<W, MyApp>(bootstrap)    ← Entry point
  └─ MyApp::window_config()         ← Read WindowConfig
  └─ window opened
  └─ bootstrap(window, services, _) ← User registers WgpuRenderSystem
  └─ MyApp::new()                   ← Simple constructor, no context
  └─ engine init                    ← Default services + DCC + agents registered
  └─ MyApp::setup(world, services)  ← Cache services, spawn entities
  └─ dcc.initialize_agents(ctx)     ← Agents cache services once

Per frame:
  app.update(world, inputs)         ← User game logic
  world.tick_maintenance()          ← ECS GC (direct, not an agent)
  dcc.execute_agents(&mut ctx)      ← Agents dispatch lanes
```

App must implement `EngineApp + AgentProvider + PhaseProvider` (composite SDK trait).

Detail in [`docs/src/03_lifecycle.md`](../docs/src/03_lifecycle.md).

## 08 — Agent lifecycle

```rust
pub trait Agent: Send + Sync {
    fn id(&self) -> AgentId;
    fn negotiate(&mut self, request: NegotiationRequest) -> NegotiationResponse;
    fn apply_budget(&mut self, budget: ResourceBudget);
    fn report_status(&self) -> AgentStatus;
    fn on_initialize(&mut self, context: &mut EngineContext<'_>) {}  // Once after registration
    fn execute(&mut self, context: &mut EngineContext<'_>);          // Every frame
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
```

The five agents — one per `LaneKind`:

| Agent | `LaneKind` | Strategies | Allowed `EngineMode` |
|---|---|---|---|
| `RenderAgent` | `Render` | Unlit / LitForward / Forward+ | `Playing`, `Custom("editor")` |
| `ShadowAgent` | `Shadow` | ShadowPassLane (atlas) | `Playing`, `Custom("editor")` |
| `PhysicsAgent` | `Physics` | Standard / Simplified | `Playing` |
| `UiAgent` | `Ui` | Layout + Render | `Custom("editor")` (editor only) |
| `AudioAgent` | `Audio` | Source count / quality | `Playing` |

`EngineMode` is `Playing` or `Custom(String)`. The base engine ships only `Playing`; the editor injects `Custom("editor")`. Do not confuse with the editor's UI-state enum `PlayMode` (`Editing` / `Playing` / `Paused`).

Detail in [`docs/src/06_agents.md`](../docs/src/06_agents.md) and [`docs/src/08_gorna.md`](../docs/src/08_gorna.md).

---

*End of architecture brief.*
