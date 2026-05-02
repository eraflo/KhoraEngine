# Roadmap

The phased development plan for Khora. Six phases, multi-year horizon.

- Document — Khora Roadmap v1.0
- Status — Living
- Date — May 2026

---

## Contents

1. Phase 1 — Foundational architecture
2. Phase 2 — Scene, assets, basic capabilities
3. Phase 3 — The adaptive core
4. Phase 4 — Tooling, usability, scripting
5. Phase 5 — Advanced intelligence
6. Phase 6 — Native physics
7. Closed milestones (historical)

---

## 01 — Phase 1 — Foundational architecture

**Goal:** Establish the complete, decoupled CLAD crate structure and render a basic scene through the SDK.

**Status:** Complete.

With the successful abstraction of command recording and submission, the core architectural goals for the foundational phase are met. The engine is fully decoupled from the rendering backend — wgpu is *one* implementation, not *the* implementation.

## 02 — Phase 2 — Scene, assets, basic capabilities

**Goal:** Build out the necessary features to represent and interact with a game world, starting with the implementation of CRPECS.

### Architecture refactoring
- **Lift `asset_lane` and `ecs_lane` out of the Lane abstraction.** Per the *Agent vs Service* rule, a `Lane` is a strategy variant an agent picks under GORNA negotiation. Asset decoders (glTF, OBJ, WAV, Symphonia, texture, font, pack) and ECS compaction have no per-frame strategies to negotiate — they are on-demand or fixed maintenance work. They should expose their behavior through the existing service surfaces (`AssetService`, `EcsMaintenance`) rather than implement `Lane`. Targets:
  - Replace `AssetDecoder<A>` *lane* implementations with plain `AssetDecoder<A>` services registered in `DecoderRegistry`. The `AssetDecoder<A>` trait already exists in `khora-lanes` without a `Lane` bound — finish moving the decoders to use it cleanly and drop the lane scaffolding.
  - Move `CompactionLane` work directly into `EcsMaintenance::tick`, deleting the lane wrapper. Maintenance is already not an agent (see [ECS](./05_ecs.md) §08); the lane wrapper is residual.
  - Update [Lanes](./07_lanes.md) and [Architecture](./02_architecture.md) tables once the migration lands — today they still list `asset_lane/` and `ecs_lane/` for accuracy with the current code, but those entries should disappear after this refacto.

### Rendering capabilities, physics, animation, AI
- #101 Implement Skeletal Animation System
- #162 Implement SkinnedMesh ComputeLane
- #104 Implement Basic AI System (placeholder behaviors, simple state machine)

## 03 — Phase 3 — The adaptive core

**Goal:** Implement the magic of Khora — the DCC, ISAs, and GORNA — proving the SAA concept.

### Intelligent Subsystem Agents v1
- #176 Evolve `AssetAgent` into a full ISA (depends on #174)
- #83 Refactor a second subsystem as ISA v0.1

## 04 — Phase 4 — Tooling, usability, scripting

**Goal:** Make the engine usable and debuggable by humans. Build the editor, provide observability tools, integrate scripting.

### Editor GUI, observability, UI
- #52 Choose and integrate a GUI library
- #53 Create the editor layout
- #54 Implement the render viewport
- #55 Implement the scene hierarchy panel
- #56 Implement the inspector panel (basic components)
- #57 Implement the performance / context visualization panel
- #58 Implement basic Play / Stop mode
- #77 Visualize the full context model in the editor debug panel
- #102 Implement the in-engine UI system
- #164 Implement `UiRenderLane`
- #165 Implement a Decision Tracer for DCC / GORNA in the editor
- #166 Implement a timeline scrubber for the context visualization panel

### Editor polish, networking, manual control
- #175 Real-time asset database for the editor (depends on #41)
- #177 `DeltaSerializationLane` for game saves and undo / redo (depends on #45)
- #66 Implement an asset browser (depends on #175)
- #67 Implement a material editor
- #68 Implement gizmos
- #167 Implement an `EditorGizmo` `RenderLane`
- #69 Implement undo / redo
- #70 Implement editor panels for fine-grained system control
- #103 Implement a basic networking system

### Scripting v1
- #168 Evaluate and choose a scripting language
- #169 Implement scripting backend and bindings
- #170 Make the scripting VM an ISA (`ScriptingAgent`)

### Maturation, optimization, packaging
- #94 Extensive performance profiling and optimization
- #95 Documentation overhaul (including SAA concepts)
- #96 Build and packaging for target platforms

### API ergonomics and developer experience
- #173 Implement a fluent API for entity creation

## 05 — Phase 5 — Advanced intelligence

**Goal:** Build upon the stable SAA foundation to explore next-generation features.

### Advanced adaptivity (AGDF, contracts)
- #89 Design semantic interfaces and contracts v1
- #90 Investigate Adaptive Game Data Flow (AGDF) feasibility and design
- #91 Implement basic AGDF for a specific component type
- #92 Explore using specialized hardware (ML cores)
- #129 Metrics system advanced features (labels, histograms, export)

### DCC v2 — developer guidance and control
- #93 Implement more sophisticated DCC heuristics, potentially ML-based decision model
- #171 Implement engine adaptation modes (Learning, Stable, Manual)
- #172 Implement developer hints and constraints system (`PriorityVolume`)

### Core XR integration
- #59 Integrate OpenXR SDK and bindings
- #60 Implement XR instance / session / space management
- #61 Integrate the graphics API with XR
- #62 Implement stereo rendering path
- #63 Implement head and controller tracking
- #64 Integrate XR performance metrics
- #65 Display a basic scene in VR with performance overlay

## 06 — Phase 6 — Native physics

**Goal:** Replace the third-party solver with a native Khora solver implementing cutting-edge physical simulation research.

### Pillar 1 — unified simulation, MPM
- #300 **Unified simulation (MLS-MPM).** Implement *MLS-MPM: Moving Least Squares Material Point Method* for unified simulation of snow, sand, and fluids. Target: pure algorithmic interaction between disparate materials.
- #301 **Sparse volume physics (NanoVDB).** Integrate NanoVDB (OpenVDB) for GPU-accelerated sparse volume simulation (fire, smoke, large-scale explosions).

### Pillar 2 — robust constraints and collision
- #302 **Incremental Potential Contact (IPC).** Integrate *Incremental Potential Contact* (Li et al. 2020) to guarantee intersection-free and inversion-free simulation. Focus: eliminating clipping in soft-bodies and high-speed collisions.
- #303 **Stable constraints (XPBD and ADMM).** Combine XPBD for stability with ADMM optimization for complex hard constraints and heterogeneous materials.

### Pillar 3 — soft-body and Gaussian dynamics
- #304 **High-speed soft bodies (Projective Dynamics).** Study Projective Dynamics for real-time muscle and flesh simulation with implicit stability.
- #305 **Differentiable and Gaussian physics.** Explore PhysGaussian and DiffTaichi for physics-integrated Gaussian splatting and differentiable simulation.

### Pillar 4 — intelligent characters and neural simulation
- #306 **Learning-based character motion (DeepMimic).** Research DeepMimic for physics-based character animation using reinforcement learning.
- #307 **Graph network simulation.** Analysis of *Learning to Simulate Complex Physics with Graph Networks* (DeepMind) for complex particle-based interactions.

### Implementation and transition
- #308 Implement Custom Khora-Solver v1 (rigid body + XPBD core)
- #309 Transition `PhysicsAgent` and lanes to the native solver
- #310 Performance match and exceed against the previous third-party backend

---

## 07 — Closed milestones (historical)

### Core foundation and basic window
- #1 Setup Project Structure and Cargo Workspace
- #2 Implement Core Math Library (Vec3, Mat4, Quat) — design for DOD / potential AGDF
- #3 Choose and Integrate a Windowing Library
- #4 Implement Basic Input System
- #5 Create Main Application Loop Structure
- #6 Display Empty Window with Basic Stats (FPS, memory)
- #7 Setup Basic Logging and Event System
- #8 Define Project Coding Standards and Formatting
- #18 Design Core Engine Interfaces and Message Passing (thinking about ISAs and DCC)
- #19 Implement Foundational Performance Monitoring Hooks (CPU timers)
- #20 Implement Basic Memory Allocation Tracking

### Rendering primitives and ISA scaffolding
- #31 Choose and Integrate a Graphics API Wrapper
- #32 Design Rendering Interface as a potential ISA
- #33 Implement Graphics Device Abstraction
- #34 Implement Swapchain Management
- #35 Implement Basic Shader System
- #36 Implement Basic Buffer / Texture Management (track VRAM usage)
- #37 Implement GPU Performance Monitoring Hooks (timestamps)
- #110 Implement Robust Graphics Backend Selection (Vulkan / DX12 / GL fallback)
- #118 Implement Basic Rendering Pipeline System
- #121 Develop Custom Bitflags Macro for Internal Engine Use
- #123 Implement Core Metrics System Backend v1 (in-memory)
- #124 Integrate VRAM Tracking into Core Metrics System
- #125 Integrate System RAM Tracking into Core Metrics System
- #38 Render a Single Triangle / Quad with Performance Timings
- #135 Advanced GPU Performance and Resize Heuristics
- #140 Implement Basic Command Recording and Submission

### Scene representation, assets, data focus
- #39 Define Khora's ECS Architecture
- #154 Implement Core ECS Data Structures (CRPECS v1)
- #155 Implement Basic Entity Lifecycle (CRPECS v1)
- #156 Implement Native Queries (CRPECS v1)
- #40 Implement Scene Hierarchy and Transform System (depends on #156)
- #41 Design Asset System with VFS and Define Core Structs
- #174 Implement VFS Packfile Builder and Runtime (depends on #41)
- #42 Implement Texture Loading and Management (depends on #174)
- #43 Implement Mesh Loading and Management (depends on #174)
- #44 Render Loaded Static Model with Basic Materials (depends on #40, #42, #43)
- #157 Implement Component Removal and Basic Garbage Collection (CRPECS v1)
- #45 Implement Basic Scene Serialization
- #99 Implement Basic Audio System (playback and management)

### Rendering capabilities, physics, animation, strategies
- #159 Implement `SimpleUnlit` `RenderLane`
- #46 Implement Camera System and Uniforms
- #47 Implement Material System
- #48 Implement Basic Lighting Models (track shader complexity / perf)
- #160 Implement `Forward+ Lighting` `RenderLane`
- #49 Implement Depth Buffering
- #50 Explore Alternative Rendering Paths and Strategies (Forward vs Deferred concept)
- #158 Implement Transversal Queries (CRPECS v1)
- #100 Implement Basic Physics System (integration and collision detection) (depends on #40)
- #161 Define and Implement Core `PhysicsLanes` (broadphase, solver)

### ISA v1 and basic adaptation
- #75 Design Initial ISA Interface Contract v0.1
- #76 Refactor one subsystem to partially implement ISA v0.1 (`RenderAgent` Base)
- #78 Implement Multiple Strategies for one key ISA (`RenderAgent`: Unlit, LitForward, ForwardPlus, Auto)
- #79 Refine ISA Interface Contract (Agent trait: negotiate, apply_budget, report_status)
- #80 Implement DCC Heuristics Engine v1 (9 heuristics in khora-control)
- #81 Implement DCC Command System to trigger ISA Strategy Switches (`GornaArbitrator` → `apply_budget` flow)
- #82 Demonstrate Automatic Renderer Strategy Switching (Auto mode + GORNA negotiation, 16 tests)
- #224 Implement `RenderLane` Resource Ownership (pipelines, buffers, bind groups; proper `on_shutdown`)
- #225 Implement Light Uniform Buffer System (`UniformRingBuffer` in khora-core, persistent GPU ring buffers for camera / lighting uniforms)

### GORNA v1
- #84 Design GORNA Protocol
- #85 Implement Resource Budgeting in DCC
- #86 Enhance ISAs to Estimate Resource Needs per Strategy (`estimate_cost` + VRAM-aware negotiate)
- #88 Demonstrate Dynamic Resource Re-allocation under Load

### DCC v1 — awareness
- #71 Design DCC Architecture
- #72 Implement DCC Core Service
- #73 Integrate Performance / Resource Metrics Collection into DCC
- #74 Implement Game State Monitoring Hook into DCC
- #128 DCC v1 Integration with Core Metrics System (`MetricStore`, `RingBuffer`, `GpuReport` ingestion)
- #163 Make CRPECS Garbage Collector an ISA
- #116 Evaluate Abstraction for Windowing / Platform System

---

*This roadmap reflects the current plan. Items move through Open → In Progress → Closed. The set of phases is stable; the contents within each phase grow as work continues.*
