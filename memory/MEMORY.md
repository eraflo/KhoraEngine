# Memory

## Current State

- **Branch**: `dev`
- **Build**: Clean (all crates compile)
- **Tests**: ~439 passing, 0 failures
- **Last major work**: SAA Architecture Refactoring + khora-io crate + Scene Workflow + Component Serialization
  - **Phase 1 — Agent trait cleanup**: Removed `update()`, added `on_initialize()`, `execute()` receives `&mut EngineContext`. Only 4 agents (Render, Physics, UI, Audio) ✅
  - **Phase 2 — Data layer cleanup**: `EcsMaintenance`, `SaaTrackingAllocator` moved to khora-core ✅
  - **Phase 3 — SDK cleanup**: `AppContext` replaces `EngineContext`, prelude cleaned ✅
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
    - Macro `register_components!` for DRY registration ✅
    - `ComponentKind` enum + `EditorState.pending_add_component` ✅
    - "Add Component" button in Properties Panel ✅
    - `add_component_to_entity()` in ops.rs ✅
    - `InspectedEntity` extended with all component snapshot fields ✅
    - Scene save/load now captures ALL components (Name, Camera, Light, RigidBody, Collider, AudioSource, UI, etc.) ✅
    - Hub no longer creates `default.scene.json` ✅
    - Scene tree rename supports Enter/Escape ✅

## Known Issues

- Vulkan semaphore validation errors still present at runtime
- Object jittering when moving camera — may be camera matrix precision or shadow-related
- egui-wgpu crate incompatible with wgpu 28.0 — custom renderer in khora-infra
- Editor unused import warnings after prelude cleanup (cosmetic, not errors)
- `transform_propagation_system` still in khora-lanes (should move to khora-data)
- `InspectedEntity` snapshot extraction only populates core fields (Transform, Camera, Light, RigidBody, Collider, AudioSource) — newer fields (physics_material, kinematic_character_controller, audio_listener, ui_*) are always false/None in the inspector

## Architecture Decisions

- **12 crates** in workspace: core, data, io, lanes, control, agents, infra, telemetry, macros, plugins, sdk, editor
- Lane trait is the universal pipeline interface for hot-path work
- **Agent vs Service rule**: 4 agents (Render, Physics, UI, Audio) — non-GORNA uses services in `khora-io`
- **khora-io**: Dedicated crate for I/O services — separates data plane from control plane
- Asset pipeline: VFS → `AssetIo` (FileLoader/PackLoader) → `AssetDecoder<A>` → `Assets<T>`
- Serialization: `SerializationStrategy` (Definition/Recipe/Archetype) via `khora-io`
- **Component serialization**: `#[derive(Component)]` generates `SerializableX` + `From` + inventory registration
- ECS maintenance: `EcsMaintenance` in `GameWorld.tick_maintenance()` — not an Agent
- CRPECS: Archetype SoA, parallel queries, semantic domains
- GORNA: Dynamic agent budget negotiation with thermal/battery multipliers
