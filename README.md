# KhoraEngine

KhoraEngine is an experimental game engine being developed in Rust. Its ultimate and ambitious goal is to implement a novel **Symbiotic Adaptive Architecture (SAA)**.

This architecture aims to create a deeply context-aware engine where subsystems act as cooperating agents (Intelligent Subsystem Agents - ISAs) under the guidance of a central coordinator (Dynamic Context Core - DCC). The engine will dynamically adapt its behavior, resource allocation, and even potentially its data structures based on real-time performance metrics, resource availability, application goals, and hardware capabilities.

## Vision: Symbiotic Adaptive Architecture (SAA)

The SAA philosophy emphasizes:

*   **Context-Oriented Design:** Collecting and utilizing state, performance, and resource data is paramount.
*   **Extreme Modularity / Semantic Interfaces:** Designing subsystems as potential adaptive agents (ISAs) with clear interfaces.
*   **Built-in Measurability:** Performance profiling, metrics, and resource tracking are fundamental, not afterthoughts.
*   **Strategic Flexibility:** Enabling subsystems to possess multiple execution strategies (e.g., performance vs. quality).

The long-term vision includes robust support for Extended Reality (XR) applications and an integrated editor, all built upon the adaptive SAA core.

## Current Status: Phase 1 - Solid Foundations & Context Awareness

**This project is nearing completion of Milestone 1: Core Foundation & Context Hooks.**

**Completed Steps:**

*   ✅ **`[Feature] Setup Project Structure & Cargo Workspace`**: Established the initial Rust project structure using a Cargo workspace (`khora_engine_core`, `sandbox`) with basic CI/CD and Readme setup.
*   ✅ **`[Feature] Implement Core Math Library (Vec3, Mat4, Quat)`**: Implemented foundational 3D math types (`Vec2`, `Vec3`, `Vec4`, `Quaternion`, `Mat3`, `Mat4`, `LinearRgba`, `AABB`) within `khora_engine_core`.
*   ✅ **`[Feature] Design Core Engine Interfaces & Message Passing (Thinking about ISAs & DCC)`**: Defined core architecture (EventBus) and initial interface traits (`Renderer`, `InputProvider` - Note: `InputProvider` trait currently unused for winit). Created `EngineEvent`, `InputEvent` types.
*   ✅ **`[Feature] Implement Basic Logging & Event System`**: Integrated `log` facade + `env_logger` and a thread-safe MPMC `EventBus` using `flume`. Added unit tests for EventBus.
*   ✅ **`[Feature] Implement Foundational Performance Monitoring Hooks (CPU Timers)`**: Added a basic `Stopwatch` utility for CPU timing. Integrated into the main loop for basic stats logging.
*   ✅ **`[Feature] Implement Basic Memory Allocation Tracking`**: Implemented heap allocation tracking using a custom global allocator (`SaaTrackingAllocator`) with robustness checks. Memory usage logged periodically.
*   ✅ **`[Feature] Choose and Integrate Windowing Library (e.g., winit)`**: Integrated `winit` (v0.30) using the `ApplicationHandler` and `run_app` model. Created a `KhoraWindow` wrapper for abstraction. Added optional `raw-window-handle` support via feature flag.
*   ✅ **`[Feature] Create Main Loop Structure`**: Implemented the main engine loop structure driven by the `winit` event loop (`run_app`, `ApplicationHandler`).
*   ✅ **`[Feature] Implement Basic Input System (Feed events into core)`**: Implemented translation logic from `winit` window events (keyboard, mouse) into internal `EngineEvent::Input` types, published via the `EventBus`. Added unit tests for translation logic.

**Current Focus / Next Steps:**

The final task for Milestone 1 is to provide visual feedback directly in the window.

*   ➡️ **`(Upcoming)`** `[Task] Display Empty Window & Basic Stats (FPS, Mem)`: Displaying the FPS and Memory usage statistics *within* the application window itself, instead of just logging to the console. This will likely involve either updating the window title or integrating a minimal text rendering solution (prelude to Milestone 2).

**Note:** This is a highly ambitious, long-term research and development project. The SAA goal requires significant R&D.

## Getting Started

The project currently sets up core components, creates a window, handles basic OS events (resize, focus, close), translates keyboard/mouse input into internal events, and logs performance/memory statistics. Unit tests cover the EventBus, input translation logic, and basic engine event handling.

```bash
# Clone the repository
git clone https://github.com/eraflo/KhoraEngine.git
cd KhoraEngine

# Check code
cargo check --workspace

# Run unit tests
# Note: Window creation and main loop logic are tested via sandbox execution.
cargo test --workspace

# Build the project
# To build with raw-window-handle support (if needed later for renderer):
# cargo build --workspace --features "khora_engine_core/rwh"
cargo build --workspace

# Run the sandbox (outputs logs to console)
# Enable trace logs for detailed event flow:
# RUST_LOG=khora_engine_core=trace cargo run --bin sandbox
# To run with raw-window-handle feature enabled:
# cargo run --bin sandbox --features "khora_engine_core/rwh"
cargo run --bin sandbox