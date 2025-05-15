      
<p align="center">
  <img src="assets/logos/khora_full_logo.png" alt="KhoraEngine Logo" width="250">
</p>


<h1 align="center">KhoraEngine</h1>
<p align="center">
    <a href="https://github.com/eraflo/KhoraEngine/actions/workflows/rust.yml"><img src="https://github.com/eraflo/KhoraEngine/actions/workflows/rust.yml/badge.svg" alt="Rust CI"/></a>
</p>
<p align="center">
    KhoraEngine is an experimental game engine being developed in Rust. Its ultimate and ambitious goal is to implement a novel <strong>Symbiotic Adaptive Architecture (SAA)</strong>.
</p>

</br>

<p align="center">
This architecture aims to create a deeply context-aware engine where subsystems act as cooperating agents (<strong>Intelligent Subsystem Agents - ISAs</strong>) under the guidance of a central coordinator (<strong>Dynamic Context Core - DCC</strong>). The engine will dynamically adapt its behavior, resource allocation—through a sophisticated process of <strong>Goal-Oriented Resource Negotiation (GORNA)</strong>—and even potentially its core data structures, for instance through <strong>Adaptive Game Data Flows (AGDF)</strong> techniques. This adaptation will be driven by real-time performance metrics, resource availability, application goals, and hardware capabilities, enabling the engine to strive for optimal performance and user experience across diverse scenarios.
</p>

## Vision: Symbiotic Adaptive Architecture (SAA)

The SAA philosophy emphasizes:

*   **Context-Oriented Design:** Collecting and utilizing state, performance, and resource data from all engine aspects is paramount. The DCC will aggregate this into a comprehensive model of the engine's operational context.
*   **Extreme Modularity / Semantic Interfaces:** Designing subsystems as potential adaptive agents (ISAs) with clear, potentially semantic interfaces (evolving towards **Semantic Contracts**). ISAs report their capabilities, current state, and resource needs.
*   **Built-in Measurability:** Performance profiling, metrics (CPU, GPU, memory, VRAM, network, I/O), and resource tracking are fundamental, not afterthoughts, providing the essential data for the DCC's decision-making.
*   **Strategic Flexibility:** Enabling ISAs to possess multiple execution strategies (e.g., performance vs. quality, different algorithms for physics or AI pathfinding). The DCC can then request strategy switches based on the overall context and goals.
*   **Goal-Oriented Resource Negotiation (GORNA):** A core mechanism where ISAs actively request resource budgets (CPU time, memory, VRAM, bandwidth) and the DCC dynamically allocates these based on global objectives, ISA priorities, and available resources, fostering true symbiotic cooperation.
*   **Adaptive Game Data Flows (AGDF) (Long-Term R&D):** Exploring the dynamic adaptation of data layouts (e.g., AoS to SoA for certain components) based on observed access patterns and performance feedback, further optimizing for the specific runtime context.
*   **Intelligent Coordination:** The DCC will employ a sophisticated **heuristics engine (potentially evolving with ML-driven models)** to analyze the comprehensive context and make informed decisions about strategy selection and resource allocation for ISAs.

The long-term vision includes robust support for **Extended Reality (XR)** applications and an integrated **editor**, all built upon and benefiting from the adaptive SAA core, allowing the engine and its tools to perform optimally across a wide range of hardware and application demands.

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
*   ✅ **`[Feature] Design Rendering Interface as potential ISA (Clear inputs, outputs, potential strategies)`**:
    *   A `RenderSystem` trait has been defined in `khora_engine_core::subsystems::renderer::traits`, establishing a clear contract for rendering operations.
    *   Input structures (`ViewInfo`, `RenderObject`, `RenderSettings`) and output structures (`RenderStats`, `RenderSystemError`) are defined.
    *   `RenderStrategy` enum introduced in `RenderSettings` for future flexibility.
    *   A `WgpuRenderer` struct implementing `RenderSystem` has been created, encapsulating `GraphicsContext` and WGPU-specific logic.
    *   The `Engine` now interacts with the rendering subsystem via `Box<dyn RenderSystem>`, ensuring modularity.
*   ✅ **`[Feature] Implement Graphics Device Abstraction`**:
    *   Largely achieved through the `RenderSystem` trait abstracting the engine from WGPU specifics.
    *   `GraphicsContext` now stores detailed adapter info (`adapter_name`, `adapter_backend`, `adapter_device_type`), active device features (`active_device_features`), and device limits (`device_limits`).
    *   Error handling for device/surface creation and runtime surface issues has been reviewed and solidified.


**Next Steps / Milestone 2 Tasks:**

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
```

## Community & Contributing

KhoraEngine is an open-source project and we welcome community involvement! We strive to be a welcoming and inclusive community.

*   Please read and adhere to our [**Code of Conduct**](CODE_OF_CONDUCT.md).
*   If you're interested in contributing, please see our [**Contributing Guidelines**](CONTRIBUTING.md).
*   For bug reports or feature suggestions, please use the [**Issues** tab](https://github.com/eraflo/KhoraEngine/issues) and try to use the provided templates.
*   For general questions, ideas, or discussions about KhoraEngine and its SAA/AGDF concepts, please join us on our [**GitHub Discussions page**](https://github.com/eraflo/KhoraEngine/discussions)!