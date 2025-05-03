      
<p align="center">
  <img src="assets/logos/khora_full_logo.png" alt="KhoraEngine Logo" width="250">
</p>

<div style="text-align: center;">
  <h1>KhoraEngine</h1>
  <p>
    KhoraEngine is an experimental game engine being developed in Rust. Its ultimate and ambitious goal is to implement a novel <strong>Symbiotic Adaptive Architecture (SAA)</strong>.
  </p>
</div>

</br>

<p align="center">
This architecture aims to create a deeply context-aware engine where subsystems act as cooperating agents (Intelligent Subsystem Agents - ISAs) under the guidance of a central coordinator (Dynamic Context Core - DCC). The engine will dynamically adapt its behavior, resource allocation, and even potentially its data structures based on real-time performance metrics, resource availability, application goals, and hardware capabilities.
</p>

## Vision: Symbiotic Adaptive Architecture (SAA)

The SAA philosophy emphasizes:

*   **Context-Oriented Design:** Collecting and utilizing state, performance, and resource data is paramount.
*   **Extreme Modularity / Semantic Interfaces:** Designing subsystems as potential adaptive agents (ISAs) with clear interfaces.
*   **Built-in Measurability:** Performance profiling, metrics, and resource tracking are fundamental, not afterthoughts.
*   **Strategic Flexibility:** Enabling subsystems to possess multiple execution strategies (e.g., performance vs. quality).

The long-term vision includes robust support for Extended Reality (XR) applications and an integrated editor, all built upon the adaptive SAA core.

## Project Status

**Phase 1, Milestone 1: Core Foundation & Context Hooks - ✅ COMPLETE**

The foundational layer of the engine is established. Key achievements include:

*   Setup of the Cargo workspace (`khora_engine_core`, `sandbox`) and basic CI/CD.
*   Implementation of core math types (`Vec2/3/4`, `Mat3/4`, `Quaternion`, etc.).
*   Definition of core architecture patterns (thread-safe `EventBus`, initial traits).
*   Integration of logging (`env_logger`) and event handling.
*   Implementation of basic CPU performance timing (`Stopwatch`).
*   Integration of heap memory allocation tracking (`SaaTrackingAllocator`).
*   Integration of windowing (`winit` via `ApplicationHandler`) and basic input event translation.
*   Establishment of the main application loop structure.

*(Git Tag: `m1-complete` marks this stage)*

---

**Current Focus: Phase 1, Milestone 2 - Rendering Primitives & ISA Scaffolding**

The project is now entering Milestone 2. The primary goal is to set up the basics of 3D rendering using a modern graphics API, structure the rendering system as a potential Agent (ISA), and integrate GPU performance measurement.

**Next Steps / Milestone 2 Tasks:**

*   ➡️ **`[Feature] Choose & Integrate Graphics API Wrapper (wgpu/ash/etc.)`**
    *   Description: Select and integrate a Rust library providing safe or unsafe bindings for a modern graphics API (Vulkan, DX12, Metal via wgpu or directly).
    *   Labels: `rendering`, `core`, `platform`
*   ➡️ **`[Feature] Design Rendering Interface as potential ISA (Clear inputs, outputs, potential strategies)`**
    *   Description: Design the internal API of the rendering system, clearly defining its inputs (scene data), outputs (rendered image), and anticipating the possibility of implementing different rendering strategies later.
    *   Labels: `rendering`, `architecture`, `saa-prep`
*   ➡️ **`[Feature] Implement Graphics Device Abstraction`**
    *   Description: Create an abstraction layer over the chosen graphics API to simplify logical and physical device management.
    *   Labels: `rendering`, `core`
*   ➡️ **`[Feature] Implement Swapchain Management`**
    *   Description: Manage the swapchain for presenting rendered images to the window.
    *   Labels: `rendering`, `platform`
*   ➡️ **`[Feature] Implement Basic Shader System`**
    *   Description: Set up a system for loading, compiling (if necessary), and managing shaders (vertex, fragment).
    *   Labels: `rendering`
*   ➡️ **`[Feature] Implement Basic Buffer/Texture Management (Track VRAM usage)`**
    *   Description: Create systems to manage the creation, uploading, and binding of buffers (vertex, index, uniform) and textures, while tracking VRAM usage.
    *   Labels: `rendering`, `performance`, `asset`, `saa-prep`
*   ➡️ **`[Feature] Implement GPU Performance Monitoring Hooks (Timestamps)`**
    *   Description: Use graphics API timestamp queries to measure the time spent by the GPU on different parts of the rendering process. Essential for SAA.
    *   Labels: `rendering`, `performance`, `infra`, `saa-prep`
*   ➡️ **`[Task] Render a Single Triangle/Quad with Performance Timings`**
    *   Description: Display a simple geometric shape using the established rendering pipeline and show the associated CPU/GPU timings.
    *   Labels: `rendering`, `performance`

**Note:** This is a highly ambitious, long-term research and development project. The SAA goal requires significant R&D.

## Getting Started

The project currently sets up core components, creates a window, handles basic OS events and input, and logs performance/memory statistics to the console.

```bash
# Clone the repository
git clone https://github.com/eraflo/KhoraEngine.git
cd KhoraEngine

# (Optional) Checkout the state after Milestone 1 completion
# git checkout m1-complete

# Check code
cargo check --workspace

# Run unit tests
cargo test --workspace

# Build the project
cargo build --workspace

# Run the sandbox (outputs logs to console)
# RUST_LOG=khora_engine_core=trace cargo run --bin sandbox
cargo run --bin sandbox