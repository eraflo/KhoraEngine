# Engine Pipelines

## Frame Lifecycle

```
SDK handle_frame()
  │
  ├─ app.update(world, inputs)          # Game logic
  │
  ├─ rs.begin_frame()                   # Acquire swapchain (ONE acquire)
  │   ├─ device.poll_device_non_blocking()
  │   ├─ device.wait_for_last_submission()
  │   ├─ handle pending resizes
  │   ├─ get_current_texture()
  │   └─ register_texture_view()
  │
  ├─ dcc.update_agents()
  │   ├─ UiAgent.update()               # Extract UI nodes
  │   └─ RenderAgent.update()           # Extract scene + render
  │       └─ rs.render_with_encoder()   # Encode to shared target
  │           ├─ Shadow lanes execute   # ShadowPassLane
  │           └─ Render lane executes   # LitForwardLane
  │
  ├─ dcc.execute_agents()
  │   └─ UiAgent.execute()              # Render UI overlay
  │       └─ rs.render_with_encoder()   # Encode to SAME target
  │           └─ UiRenderLane (LoadOp::Load)
  │
  └─ rs.end_frame()                     # Present (ONE present)
      └─ surface_texture.present()
```

## Rendering Pipeline

### Render Strategies (per-frame switching via GORNA)

| Strategy | Lane | Description |
|----------|------|-------------|
| Unlit | `SimpleUnlitLane` | No lighting, baseline |
| Forward | `LitForwardLane` | PBR with per-light passes, shadow sampling (PCF 3×3) |
| Forward+ | `ForwardPlusLane` | Tile-based light culling, many lights |
| Shadow | `ShadowPassLane` | Depth-only shadow map rendering |
| UI | `UiRenderLane` | 2D UI primitives |
| Extract | `ExtractLane` | ECS → GPU-ready data transfer |

### Shadow System

- `ShadowPassLane` renders to a 2048×2048 Depth32Float atlas (4 layers)
- Directional lights: orthographic projection from camera frustum AABB in light space
- Texel snapping: ortho bounds rounded to texel-aligned boundaries to prevent shimmer
- Shadow sampling: PCF 3×3 in `lit_forward.wgsl` with comparison sampler
- Shadow data flows via `LaneContext`: `ShadowAtlasView` + `ShadowComparisonSampler`

### Shader Files (WGSL)

| Shader | Path | Purpose |
|--------|------|---------|
| `lit_forward.wgsl` | `khora-lanes/src/render_lane/shaders/` | PBR lit material with shadows |
| `shadow_depth.wgsl` | same | Depth-only shadow pass |
| `simple_unlit.wgsl` | same | Basic unlit material |
| `standard_pbr.wgsl` | same | PBR material model |
| `forward_plus.wgsl` | same | Forward+ light culling |
| `ui.wgsl` | same | UI rendering |

### GPU Resource Management

All resources accessed through typed IDs (`TextureId`, `BufferId`, `PipelineId`, etc.).
`WgpuDevice` manages creation, destruction, and lifetime tracking.
`SubmissionIndex` stored per submit for GPU sync via `wait_for_last_submission()`.

## Physics Pipeline

```
ECS (RigidBody, Collider, GlobalTransform)
  ↓ sync to PhysicsProvider
StandardPhysicsLane::execute()
  ↓ PhysicsProvider::step(dt)
  ↓ sync back to ECS (updated positions/rotations)
PhysicsDebugLane (optional: visualize collision shapes)
```

- Fixed timestep with accumulator pattern
- Rapier3D backend implements `PhysicsProvider` trait
- CCD (Continuous Collision Detection) support
- `BodyType`: Dynamic, Static, Kinematic

## Audio Pipeline

```
ECS (AudioSource, AudioListener, GlobalTransform)
  ↓
SpatialMixingLane::execute()
  ↓ distance attenuation, directional mixing
AudioDevice::start() → callback fills output buffer
```

- CPAL backend implements `AudioDevice` trait
- Spatial attenuation based on listener/source positions
- WAV, Ogg, MP3, FLAC loaders via Symphonia

## Asset Pipeline

```
VirtualFileSystem (UUID → AssetMetadata, O(1))
  ↓ AssetAgent coordinates loading
Asset Loader Lanes:
  ├─ gltf_loader_lane.rs    → Meshes, scenes
  ├─ obj_loader_lane.rs     → OBJ meshes
  ├─ wav_loader_lane.rs     → WAV audio
  ├─ symphonia_loader_lane  → Ogg/MP3/FLAC
  ├─ texture_loader_lane    → Images
  ├─ font_loader_lane       → Fonts
  └─ pack_loader.rs         → .pack archives
  ↓
Assets<T> registry (AssetHandle<T> references)
```

## UI Pipeline

```
ECS (UiTransform, UiColor, UiText, UiImage, UiBorder)
  ↓ StandardUiLane (Taffy layout computation)
UiScene (ExtractedUiNode[], ExtractedUiText[])
  ↓ UiRenderLane (rasterize to screen)
```

- Taffy layout engine (`TaffyLayoutSystem` implements `LayoutSystem`)
- `StandardTextRenderer` for glyph cache + atlas

## Serialization Pipeline

```
SerializationAgent selects strategy based on SerializationGoal:
  ├─ HumanReadableDebug → DefinitionSerializationLane (YAML/JSON)
  ├─ LongTermStability  → DefinitionSerializationLane
  └─ Performance        → RecipeSerializationLane / ArchetypeSerializationLane

SceneFile = SceneHeader + SerializedPage[]
```

## Scene Pipeline

```
TransformPropagationLane:
  Parent hierarchy → compute GlobalTransform for all entities
```

## ECS Maintenance

```
CompactionLane:
  Defragment archetype pages → improve cache locality
GarbageCollectorAgent:
  Detect orphan entities → cleanup
```

## DCC / GORNA Cold Path

```
DccService (per-tick, cold path):
  1. Health Check → poll agent status
  2. Negotiation → NegotiationRequest to each agent
  3. Fitting → GornaArbitrator solves global budget
  4. Budget Issuance → apply thermal/battery multipliers
  5. Death Spiral Detection → HeuristicEngine monitors
```

## Telemetry

| Monitor | Tracks |
|---------|--------|
| `GpuMonitor` | GPU utilization, frame timings |
| `MemoryMonitor` | Heap/resident memory |
| `VramMonitor` | Video memory usage |
| `SaaTrackingAllocator` | Per-allocation heap tracking |
