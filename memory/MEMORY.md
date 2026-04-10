# Memory

## Current State

- **Branch**: `dev`
- **Build**: Clean (all crates compile)
- **Tests**: ~439 passing, 0 failures
- **Last major work**: SAA Lifecycle Refactoring (Phases 1-8)
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
- `transform_propagation_system` still in khora-lanes (should move to khora-data)
- `InspectedEntity` snapshot extraction only populates core fields — newer fields always false/None

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
