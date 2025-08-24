# 08 - Roadmap & Issue Tracker

This document outlines the phased development plan for Khora. It integrates all open and proposed tasks into a structured series of milestones.

---

## Phase 1: Foundational Architecture
**Goal:** Establish the complete, decoupled CLAD crate structure and render a basic scene through the SDK.
**(Complete)**

*With the successful abstraction of command recording and submission, the core architectural goals for the foundational phase are now met. The engine is fully decoupled from the rendering backend.*

---

## Phase 2: Scene, Assets & Basic Capabilities
**Goal:** Build out the necessary features to represent and interact with a game world, starting with the implementation of our revolutionary ECS.

#### [Scene Representation, Assets & Data Focus]
- #42 [Feature] Implement Texture Loading & Management (Depends on #174)
- #43 [Feature] Implement Mesh Loading & Management (Depends on #174)
- #44 [Task] Render Loaded Static Model with Basic Materials (Depends on #40, #42, #43)
- #157 [Task] Implement Component Removal & Basic Garbage Collection (CRPECS v1)
- #45 [Feature] Implement Basic Scene Serialization
- #99 [Feature] Implement Basic Audio System (Playback & Management)
- #126 [Task] Integrate CPU/GPU Timers with Core Metrics System

#### [Rendering Capabilities, Physics, Animation, AI & Strategy Exploration]
- #159 [Feature] Implement SimpleUnlit RenderLane
- #46 [Feature] Implement Camera System & Uniforms
- #47 [Feature] Implement Material System
- #48 [Feature] Implement Basic Lighting Models (Track shader complexity/perf)
- #160 [Feature] Implement Forward+ Lighting RenderLane
- #49 [Feature] Implement Depth Buffering
- #50 [Research] Explore Alternative Rendering Paths/Strategies (e.g., Forward vs Deferred concept)
- #100 [Feature] Implement Basic Physics System (Integration & Collision Detection) (Depends on #40)
- #161 [Feature] Define and Implement Core PhysicsLanes (Broadphase, Solver)
- #101 [Feature] Implement Skeletal Animation System
- #158 [Feature] Implement Transversal Queries (CRPECS v1)
- #162 [Feature] Implement SkinnedMesh ComputeLane
- #104 [Feature] Implement Basic AI System (Placeholder Behaviors, e.g., Simple State Machine)

---

## Phase 3: The Adaptive Core
**Goal:** Implement the "magic" of Khora: the DCC, ISAs, and GORNA, proving the SAA concept.

#### [Dynamic Context Core (DCC) v1 - Awareness]
- #71 [Feature] Design DCC Architecture
- #72 [Feature] Implement DCC Core Service
- #73 [Feature] Integrate Performance/Resource Metrics Collection into DCC
- #74 [Feature] Implement Game State Monitoring Hook into DCC
- #75 [Feature] Design Initial ISA Interface Contract v0.1
- #76 [Task] Refactor one Subsystem to partially implement ISA v0.1
- #116 [Research/Refactor] Evaluate Abstraction for Windowing/Platform System
- #128 [Feature] DCC v1 Integration with Core Metrics System
- #163 [Feature] Make CRPECS Garbage Collector an ISA

#### [Intelligent Subsystem Agents (ISA) v1 & Basic Adaptation]
- #176 [Feature] Evolve AssetAgent into a full ISA (Depends on #174)
- #78 [Feature] Implement Multiple Strategies for one key ISA
- #79 [Feature] Refine ISA Interface Contract
- #80 [Feature] Implement DCC Heuristics Engine v1
- #81 [Feature] Implement DCC Command System to trigger ISA Strategy Switches
- #82 [Task] Demonstrate Automatic Renderer Strategy Switching
- #83 [Task] Refactor a Second Subsystem as ISA v0.1

#### [Goal-Oriented Resource Negotiation (GORNA) v1]
- #84 [Research] Design GORNA Protocol
- #85 [Feature] Implement Resource Budgeting in DCC
- #86 [Feature] Enhance ISAs to Estimate Resource Needs per Strategy
- #87 [Feature] Implement Basic GORNA between DCC and 2-3 ISAs
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
- #118 [Feature] Implement Basic Render Pipeline System        
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