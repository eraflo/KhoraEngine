# 09 - Roadmap & Issue Tracker

This document outlines the phased development plan for Khora. It integrates all open and proposed tasks into a structured series of milestones.

---

## Phase 1: Foundational Architecture
**Goal:** Establish the complete, decoupled CLAD crate structure and render a basic scene through the SDK.
**(Complete)**

*With the successful abstraction of command recording and submission, the core architectural goals for the foundational phase are now met. The engine is fully decoupled from the rendering backend.*

---

## Phase 2: Scene, Assets & Basic Capabilities
**Goal:** Build out the necessary features to represent and interact with a game world, starting with the implementation of our revolutionary ECS.

#### [Rendering Capabilities, Physics, Animation, AI & Strategy Exploration]
- #101 [Feature] Implement Skeletal Animation System
- #162 [Feature] Implement SkinnedMesh ComputeLane
- #104 [Feature] Implement Basic AI System (Placeholder Behaviors, e.g., Simple State Machine)

---

## Phase 3: The Adaptive Core
**Goal:** Implement the "magic" of Khora: the DCC, ISAs, and GORNA, proving the SAA concept.

#### [Dynamic Context Core (DCC) v1 - Awareness]
- #116 [Research/Refactor] Evaluate Abstraction for Windowing/Platform System

#### [Intelligent Subsystem Agents (ISA) v1 & Basic Adaptation]
- #176 [Feature] Evolve AssetAgent into a full ISA (Depends on #174)
- #83 [Task] Refactor a Second Subsystem as ISA v0.1

#### [Goal-Oriented Resource Negotiation (GORNA) v1]
- #88 [Task] Demonstrate Dynamic Resource Re-allocation under Load

---

## Phase 4: Tooling, Usability & Scripting
**Goal:** Make the engine usable and debuggable by humans. Build the editor, provide observability tools, and integrate a scripting language.

#### [Editor GUI, Observability & UI]
- #52 [Feature] Choose & Integrate GUI Library
- #53 [Feature] Create Editor Layout
- #54 [Feature] Implement Render Viewport
- #55 [Feature] Implement Scene Hierarchy Panel
- #56 [Feature] Implement Inspector Panel (Basic Components)
- #57 [Feature] Implement Performance/Context Visualization Panel
- #58 [Feature] Implement Basic Play/Stop Mode
- #77 [Feature] Visualize Full Context Model in Editor Debug Panel
- #102 [Feature] Implement In-Engine UI System
- #164 [Feature] Implement UiRenderLane
- #165 [Feature] Implement a "Decision Tracer" for DCC/GORNA in the editor
- #166 [Feature] Implement a Timeline Scrubber for the Context Visualization Panel

#### [Editor Polish, Networking & Manual Control]
- #175 [Feature] Implement Real-time Asset Database for Editor (Depends on #41)
- #177 [Feature] Implement DeltaSerializationLane for Game Saves & Undo/Redo (Depends on #45)
- #66 [Feature] Implement Asset Browser (Depends on #175)
- #67 [Feature] Implement Material Editor
- #68 [Feature] Implement Gizmos
- #167 [Feature] Implement EditorGizmo RenderLane
- #69 [Feature] Implement Undo/Redo Functionality
- #70 [Feature] Implement Editor Panels for Fine-Grained System Control
- #103 [Feature] Implement Basic Networking System

#### [Scripting v1]
- #168 [Research] Evaluate and choose a scripting language
- #169 [Feature] Implement Scripting Backend and Bindings
- #170 [Feature] Make the Scripting VM an ISA (`ScriptingAgent`)

#### [Maturation, Optimization & Packaging]
- #94 [Task] Extensive Performance Profiling & Optimization
- #95 [Task] Documentation Overhaul (Including SAA concepts)
- #96 [Task] Build & Packaging for Target Platforms

#### [Milestone: API Ergonomics & DX (Developer Experience)]
- #173 [Feature] Implement a Fluent API for Entity Creation

---

## Phase 5: Advanced Intelligence & Future Capabilities
**Goal:** Build upon the stable SAA foundation to explore next-generation features and specializations.

#### [Advanced Adaptivity & Specialization (AGDF, Contracts)]
- #89 [Research] Design Semantic Interfaces & Contracts v1
- #90 [Research] Investigate Adaptive Game Data Flow (AGDF) Feasibility & Design
- #91 [Prototype] Implement basic AGDF for a specific component type
- #92 [Research] Explore using Specialized Hardware (ML cores?)
- #129 [Feature] Metrics System - Advanced Features (Labels, Histograms, Export)

#### [DCC v2 - Developer Guidance & Control]
- #93 [Feature] Implement more Sophisticated DCC Heuristics / potentially ML-based Decision Model
- #171 [Feature] Implement Engine Adaptation Modes (Learning, Stable, Manual)
- #172 [Feature] Implement Developer Hints and Constraints System (`PriorityVolume`)

#### [Core XR Integration & Context]
- #59 [Feature] Integrate OpenXR SDK & Bindings
- #60 [Feature] Implement XR Instance/Session/Space Management
- #61 [Feature] Integrate Graphics API with XR
- #62 [Feature] Implement Stereo Rendering Path
- #63 [Feature] Implement Head/Controller Tracking
- #64 [Feature] Integrate XR Performance Metrics
- #65 [Task] Display Basic Scene in VR with Performance Overlay

---

## Phase 6: Next-Generation Custom Physics
**Goal:** Replace the 3rd-party solver with a native Khora solver implementing cutting-edge physical simulation research.

#### [Pillar 1: Unified Simulation & Material Point Method]
- #300 [Research] **Unified Simulation (MLS-MPM)**
    > [!TIP]
    > Implement "MLS-MPM: Moving Least Squares Material Point Method" for unified simulation of snow, sand, and fluids. Target: Pure algorithmic interaction between disparate materials.
- #301 [Research] **Sparse Volume Physics (NanoVDB)**
    > [!NOTE]
    > Integrate NanoVDB (OpenVDB) for GPU-accelerated sparse volume simulation (fire, smoke, large-scale explosions).

#### [Pillar 2: Robust Constraints & Collision (The End of Clipping)]
- #302 [Research] **Incremental Potential Contact (IPC)**
    > [!IMPORTANT]
    > Integrate ["Incremental Potential Contact (Li et al. 2020)"](https://ipc-sim.github.io/) to guarantee intersection-free and inversion-free simulation. Focus: Eliminating clipping in soft-bodies and high-speed collisions.
- #303 [Research] **Stable Constraints (XPBD & ADMM)**
    > [!NOTE]
    > Combine [XPBD](https://matthias-research.github.io/pages/publications/XPBD.pdf) for stability with [ADMM optimization](https://rahulnarain.net/publications/admm_sca16.pdf) for complex, hard constraints and heterogeneous materials.

#### [Pillar 3: Soft-Body & Gaussian Dynamics]
- #304 [Research] **High-Speed Soft Bodies (Projective Dynamics)**
    > [!NOTE]
    > Study Projective Dynamics for real-time muscle and "flesh" simulation with implicit stability.
- #305 [Research] **Differentiable & Gaussian Physics**
    > [!NOTE]
    > Explore [PhysGaussian](https://xpandora.github.io/PhysGaussian/) and [DiffTaichi](https://arxiv.org/abs/1910.00935) for physics-integrated Gaussian splatting and differentiable simulation.

#### [Pillar 4: Intelligent Characters & Neural Simulation]
- #306 [Research] **Learning-Based Character Motion (DeepMimic)**
    > [!TIP]
    > Research [DeepMimic (Arxiv)](https://arxiv.org/abs/1804.02717) for physics-based character animation using Reinforcement Learning.
- #307 [Research] **Graph Network Simulation (DeepMind)**
    > [!NOTE]
    > Analysis of ["Learning to Simulate Complex Physics with Graph Networks"](https://arxiv.org/abs/2002.09405) for complex particle-based interactions.

#### [Implementation & Transition]
- #308 [Feature] Implement Custom Khora-Solver v1 (Rigid Body + XPBD Core)
- #309 [Feature] Transition PhysicsAgent & Lanes to Native Solver
- #310 [Task] Performance Match & Exceed against previous 3rd-party backend

---

## Closed Issues (Historical Reference)

### [Core Foundation & Basic Window]
- #1 [Feature] Setup Project Structure & Cargo Workspace
- #2 [Feature] Implement Core Math Library (Vec3, Mat4, Quat) - Design for DOD/Potential ADF
- #3 [Feature] Choose and Integrate Windowing Library
- #4 [Feature] Implement Basic Input System (Feed events into core)
- #5 [Feature] Create Main Application Loop Structure
- #6 [Task] Display Empty Window & Basic Stats (FPS, Mem)
- #7 [Task] Setup Basic Logging & Event System
- #8 [Task] Define Project Coding Standards & Formatting
- #18 [Feature] Design Core Engine Interfaces & Message Passing (Thinking about ISAs & DCC)
- #19 [Feature] Implement Foundational Performance Monitoring Hooks (CPU Timers)
- #20 [Feature] Implement Basic Memory Allocation Tracking

### [Core Foundation & Context Hooks]
- #21 [Feature] Setup Project Structure & Cargo Workspace
- #22 [Feature] Implement Core Math Library (Vec3, Mat4, Quat) - Design for DOD/Potential AGDF
- #23 [Feature] Design Core Engine Interfaces & Message Passing (Thinking about ISAs & DCC)
- #24 [Feature] Implement Basic Logging & Event System
- #25 [Feature] Implement Foundational Performance Monitoring Hooks (CPU Timers)
- #26 [Feature] Implement Basic Memory Allocation Tracking
- #27 [Feature] Choose and Integrate Windowing Library
- #28 [Feature] Implement Basic Input System (Feed events into core)  
- #29 [Feature] Create Main Loop Structure (placeholder for future DCC control)
- #30 [Task] Display Empty Window & Basic Stats (FPS, Mem)

### [Rendering Primitives & ISA Scaffolding]
- #31 [Feature] Choose & Integrate Graphics API Wrapper
- #32 [Feature] Design Rendering Interface as potential ISA (Clear inputs, outputs, potential strategies)
- #33 [Feature] Implement Graphics Device Abstraction
- #34 [Feature] Implement Swapchain Management
- #35 [Feature] Implement Basic Shader System
- #36 [Feature] Implement Basic Buffer/Texture Management (Track VRAM usage)
- #37 [Feature] Implement GPU Performance Monitoring Hooks (Timestamps)
- #110 [Feature] Implement Robust Graphics Backend Selection (Vulkan/DX12/GL Fallback)
- #118 [Feature] Implement Basic Rendering Pipeline System
- #121 [Feature] Develop Custom Bitflags Macro for Internal Engine Use
- #123 [Feature] Implement Core Metrics System Backend v1 (In-Memory)
- #124 [Task] Integrate VRAM Tracking into Core Metrics System 
- #125 [Task] Integrate System RAM Tracking into Core Metrics System
- #38 [Task] Render a Single Triangle/Quad with Performance Timings
- #135 [Enhancement] Advanced GPU Performance & Resize Heuristics
- #140 [Feature] Implement Basic Command Recording & Submission

#### [Scene Representation, Assets & Data Focus]
- #39 [Research & Design] Define Khora's ECS Architecture
- #154 [Task] Implement Core ECS Data Structures (CRPECS v1)
- #155 [Task] Implement Basic Entity Lifecycle (CRPECS v1)
- #156 [Task] Implement Native Queries (CRPECS v1)
- #40 [Feature] Implement Scene Hierarchy & Transform System (Depends on #156)
- #41 [Design] Design Asset System with VFS & Define Core Structs
- #174 [Feature] Implement VFS Packfile Builder & Runtime (Depends on #41)
- #42 [Feature] Implement Texture Loading & Management (Depends on #174)
- #43 [Feature] Implement Mesh Loading & Management (Depends on #174)
- #44 [Task] Render Loaded Static Model with Basic Materials (Depends on #40, #42, #43)
- #157 [Task] Implement Component Removal & Basic Garbage Collection (CRPECS v1)
- #45 [Feature] Implement Basic Scene Serialization
- #99 [Feature] Implement Basic Audio System (Playback & Management)

#### [Rendering Capabilities, Physics, Animation, AI & Strategy Exploration]
- #159 [Feature] Implement SimpleUnlit RenderLane
- #46 [Feature] Implement Camera System & Uniforms
- #47 [Feature] Implement Material System
- #48 [Feature] Implement Basic Lighting Models (Track shader complexity/perf)
- #160 [Feature] Implement Forward+ Lighting RenderLane
- #49 [Feature] Implement Depth Buffering
- #50 [Research] Explore Alternative Rendering Paths/Strategies (e.g., Forward vs Deferred concept)
- #158 [Feature] Implement Transversal Queries (CRPECS v1)
- #100 [Feature] Implement Basic Physics System (Integration & Collision Detection) (Depends on #40)
- #161 [Feature] Define and Implement Core PhysicsLanes (Broadphase, Solver)

#### [Intelligent Subsystem Agents (ISA) v1 & Basic Adaptation]
- #75 [Feature] Design Initial ISA Interface Contract v0.1
- #76 [Task] Refactor one Subsystem to partially implement ISA v0.1 (RenderAgent Base)
- #78 [Feature] Implement Multiple Strategies for one key ISA (RenderAgent: Unlit, LitForward, ForwardPlus, Auto)
- #79 [Feature] Refine ISA Interface Contract (Agent trait: negotiate, apply_budget, report_status)
- #80 [Feature] Implement DCC Heuristics Engine v1 (9 heuristics in khora-control)
- #81 [Feature] Implement DCC Command System to trigger ISA Strategy Switches (GornaArbitrator → apply_budget flow)
- #82 [Task] Demonstrate Automatic Renderer Strategy Switching (Auto mode + GORNA negotiation, 16 tests)
- #224 [Feature] Implement RenderLane Resource Ownership (Pipelines, buffers, bind groups; proper on_shutdown)
- #225 [Feature] Implement Light Uniform Buffer System (UniformRingBuffer in khora-core, persistent GPU ring buffers for camera/lighting uniforms)

#### [Goal-Oriented Resource Negotiation (GORNA) v1]
- #84 [Research] Design GORNA Protocol
- #85 [Feature] Implement Resource Budgeting in DCC
- #86 [Feature] Enhance ISAs to Estimate Resource Needs per Strategy (estimate_cost + VRAM-aware negotiate)

#### [Dynamic Context Core (DCC) v1 - Awareness]
- #71 [Feature] Design DCC Architecture
- #72 [Feature] Implement DCC Core Service
- #73 [Feature] Integrate Performance/Resource Metrics Collection into DCC
- #74 [Feature] Implement Game State Monitoring Hook into DCC
- #128 [Feature] DCC v1 Integration with Core Metrics System (MetricStore, RingBuffer, GpuReport ingestion)
- #163 [Feature] Make CRPECS Garbage Collector an ISA
