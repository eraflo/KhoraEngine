      
<p align="center">
  <img src="assets/logos/khora_full_logo.png" alt="KhoraEngine Logo" width="250">
</p>


<h1 align="center">KhoraEngine</h1>
<p align="center">
    KhoraEngine is an experimental game engine being developed in Rust. Its ultimate and ambitious goal is to implement a novel <strong>Symbiotic Adaptive Architecture (SAA)</strong>.
</p>

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

*(Git Tag: `v0.1.0` marks this stage)*

---

**Current Focus: Phase 1, Milestone 2 - Rendering Primitives & ISA Scaffolding**

Development is underway on Milestone 2, focusing on establishing basic rendering capabilities.

**Completed M2 Tasks:**

*   ✅ **`[Feature] Choose & Integrate Graphics API Wrapper (wgpu/ash/etc.)`**:
    *   `wgpu` (v0.20) was chosen and integrated as the graphics API wrapper.
    *   A `GraphicsContext` struct has been implemented within `khora_engine_core::subsystems::renderer` to manage core `wgpu` objects (Instance, Surface, Adapter, Device, Queue).
    *   The graphics context is successfully initialized after window creation and stored within the `Engine`.
    *   The `wgpu::Surface` is configured and correctly handles window resizing.
    *   A basic render function is implemented within `GraphicsContext` that successfully acquires a frame, performs a clear operation, and handles presentation.
    *   **Workaround:** Due to severe, system-specific swapchain blocking/timeout issues encountered with the Vulkan and DX12 backends on the development machine (NVIDIA Optimus setup), the **OpenGL backend (`wgpu::Backends::GL`) is currently forced** during instance creation for compatibility and stable operation. Robust backend selection with fallback is planned for later (see new issue below).
    *   SAA Prep: Requested the `TIMESTAMP_QUERY` feature during device creation to prepare for future GPU performance monitoring.


**Next Steps / Milestone 2 Tasks:**

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
*   ➡️ **`[Feature] Implement Robust Graphics Backend Selection (Vulkan/DX12/GL Fallback)`**
    *   Description: Enhance the graphics initialization process (`GraphicsContext::new` or similar) to attempt initializing backends in a preferred order (e.g., Vulkan, DX12 on Windows, Metal on macOS) and gracefully fall back to the next option (e.g., OpenGL/GLES via ANGLE) if the preferred backend fails to initialize or is known to be problematic on the detected hardware/driver combination (based on future heuristics or stored knowledge). This improves engine robustness across diverse hardware and drivers, aligning with the SAA principle of adapting to the execution context. Report the chosen backend via logs.
    *   Labels: `rendering`, `core`, `platform`, `robustness`, `saa-prep`
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