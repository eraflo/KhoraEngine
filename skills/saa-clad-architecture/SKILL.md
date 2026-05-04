---
name: saa-clad-architecture
description: "Symbiotic Adaptive Architecture (SAA) and CLAD layering — Control/Lanes/Agents/Data architecture, DCC orchestration, GORNA protocol, agent lifecycle, budget system, and inter-agent communication. Use for architectural decisions, agent implementation, or DCC/GORNA work."
license: Apache-2.0
metadata:
  author: eraflo
  version: "1.0.0"
  category: engine-architecture
---

# SAA / CLAD Architecture

## Instructions

When working on the engine's architecture:

1. **CLAD Layers** (strict dependency order, no upward references):
   - **[C]ontrol** (`khora-control`) — DCC orchestration, GORNA protocol, agent scheduling
   - **[L]anes** (`khora-lanes`) — deterministic hot-path pipelines (render, physics, audio)
   - **[A]gents** (`khora-agents`) — intelligent subsystem managers (RenderAgent, AudioAgent, etc.)
   - **[D]ata** (`khora-data`) — ECS (CRPECS), allocators, resources

2. **DCC (Dynamic Capability Controller)** (`crates/khora-control/src/`):
   - Manages agent lifecycle: registration, scheduling, execution
   - Agents sorted by priority (descending) for execution order
   - Two phases per frame: `update_agents()` then `execute_agents()`
   - Each agent receives an `EngineContext` with world access and services

3. **GORNA Protocol** (Goal-Oriented Resource Negotiation Architecture):
   - Agents negotiate for shared resources (GPU time, memory, etc.)
   - Budget system: agents request budgets, DCC allocates based on priority
   - `GornaBudget` enum: `HighFidelity`, `Balanced`, `LowPower`, `Auto`

4. **Agent trait** (`khora-core`):
   ```rust
   pub trait Agent: Send + Sync {
       fn id(&self) -> AgentId;
       fn negotiate(&mut self, request: NegotiationRequest) -> NegotiationResponse;
       fn apply_budget(&mut self, budget: ResourceBudget);
       fn update(&mut self, context: &mut EngineContext<'_>);
       fn execute(&mut self);
       fn report_status(&self) -> AgentStatus;
       fn as_any(&self) -> &dyn Any;
       fn as_any_mut(&mut self) -> &mut dyn Any;
   }
   ```

5. **Current agents** (`crates/khora-agents/src/`):
   - `RenderAgent` — scene extraction (ECS → RenderWorld), polymorphic render strategy selection (Unlit/Forward/Forward+), GORNA negotiation
   - `UiAgent` — UI layout computation via Taffy, UiScene extraction, overlay rendering
   - `PhysicsAgent` — physics simulation configuration, PhysicsProvider management, ECS ↔ physics sync
   - `AudioAgent` — audio device management, spatial mixing orchestration (SAA violation: does pipeline work directly)
   - `AssetAgent` — asset loading coordination, VFS integration, caching, hot-reload
   - `SerializationAgent` — scene save/load, strategy selection by SerializationGoal (SAA violation: bypasses Lane)
   - `GarbageCollectorAgent` — entity cleanup, orphan detection (SAA violation: moderate direct work)

6. **Frame flow** (SDK → DCC → Agents):
   ```
   handle_frame()
     ├─ app.update(world, inputs)
     ├─ begin_frame()              ← acquire swapchain
     ├─ dcc.update_agents()        ← agents extract/prepare
     ├─ dcc.execute_agents()       ← agents encode/render
     └─ end_frame()                ← present
   ```

7. **Architecture rules**:
   - Agents must NOT directly reference other agents
   - Communication goes through `LaneContext`, ECS, or `ServiceRegistry`
   - Lanes are pure pipelines — no agent logic, no ECS queries
   - Data layer has zero knowledge of rendering or control

## Key Subsystems Overview

- **Physics**: `PhysicsProvider` trait → Rapier3D backend, RigidBody/Collider sync, CCD, fixed timestep
- **Audio**: `AudioDevice` trait → CPAL backend, `SpatialMixingLane` for 3D positional audio
- **Assets/VFS**: `VirtualFileSystem` (UUID → metadata O(1)), loaders (glTF, OBJ, WAV, Ogg/MP3/FLAC, textures, fonts), `.pack` archives
- **UI**: Taffy layout, `UiTransform`/`UiColor`/`UiText`/`UiImage`/`UiBorder`, `StandardUiLane` → `UiRenderLane`
- **Serialization**: 3 strategies (Definition/human-readable, Recipe/compact, Archetype/binary)
- **Telemetry**: `TelemetryService`, GpuMonitor, MemoryMonitor, VramMonitor, SaaTrackingAllocator
- **Input**: winit → `InputEvent` (keyboard, mouse, scroll) via `translate_winit_input()`

## Known Violations (tracked for refactoring)

- `AudioAgent` does pipeline work directly (should use `SpatialMixingLane`)
- `SerializationAgent` bypasses Lane abstraction
- `GarbageCollectorAgent` does moderate direct work (should be Lane-based)
