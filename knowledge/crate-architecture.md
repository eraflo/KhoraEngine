# Crate Architecture

## CLAD Dependency Graph

```
khora-sdk
├── khora-agents
│   ├── khora-lanes
│   │   ├── khora-core
│   │   └── khora-data
│   ├── khora-infra
│   │   ├── khora-core
│   │   └── khora-data
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

## Crate Responsibilities

| Crate | CLAD Layer | Responsibility |
|-------|-----------|----------------|
| `khora-core` | Foundation | Traits (Lane, Agent, RenderSystem, PhysicsProvider, AudioDevice, LayoutSystem, Asset, VFS), math (Vec2/3/4, Mat3/4, Quat, Aabb, LinearRgba), GORNA types, error hierarchy, ServiceRegistry, EngineContext |
| `khora-macros` | Foundation | `#[derive(Component)]` proc macro |
| `khora-data` | **D**ata | CRPECS ECS (World, Archetype, Query, Page, SemanticDomain), SaaTrackingAllocator, Assets<T> storage, UI components, scene definitions |
| `khora-control` | **C**ontrol | DccService (agent lifecycle), GornaArbitrator (budget fitting), HeuristicEngine (death spiral detection), Context (thermal/battery/execution phase) |
| `khora-telemetry` | Infra | TelemetryService, MetricsRegistry, MonitorRegistry, telemetry event storage |
| `khora-lanes` | **L**anes | Render (Unlit, LitForward, Forward+, Shadow, UI, Extract), Physics (Standard, Debug), Audio (SpatialMixing), Asset (glTF, OBJ, WAV, Symphonia, Texture, Font, Pack), ECS (Compaction), Scene (Definition, Recipe, Archetype serialization, TransformPropagation), UI (StandardUi) |
| `khora-infra` | Infra | WgpuRenderSystem/WgpuDevice (GPU), WinitWindow (window), input translation, Rapier3D (physics), CPAL (audio), Taffy (layout), GpuMonitor/MemoryMonitor/VramMonitor |
| `khora-agents` | **A**gents | RenderAgent, UiAgent, PhysicsAgent, AudioAgent, AssetAgent, SerializationAgent, GarbageCollectorAgent |
| `khora-plugins` | Extension | Plugin loading and registration system |
| `khora-sdk` | Public API | Engine entry point, GameWorld (safe ECS facade), Application trait, Vessel primitives (cube, sphere, plane) |
| `khora-editor` | Application | Future editor (currently a stub) |

## Key Rule

Dependencies flow downward only: SDK → Agents → Lanes → Data/Core. Never upward.

## Core Trait Map

| Trait | Crate | Implemented By |
|-------|-------|---------------|
| `Lane` | khora-core | All lane types in khora-lanes |
| `Agent` | khora-core | All agent types in khora-agents |
| `RenderSystem` | khora-core | `WgpuRenderSystem` in khora-infra |
| `PhysicsProvider` | khora-core | Rapier3D backend in khora-infra |
| `AudioDevice` | khora-core | CPAL backend in khora-infra |
| `LayoutSystem` | khora-core | `TaffyLayoutSystem` in khora-infra |
| `Asset` | khora-core | All loadable asset types |
| `Component` | khora-data | All ECS components (via derive macro) |

## Standard Components (khora-data)

| Component | Domain | Purpose |
|-----------|--------|---------|
| `Transform` | All | Local position/rotation/scale |
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
