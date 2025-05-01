# KhoraEngine

<!-- Badges placeholder: Add build status, license, CI status, etc. later -->
<!-- [![Build Status](...)](...) -->
<!-- [![License](...)](...) -->

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

**This project is progressing through the initial steps of Milestone 1: Core Foundation & Context Hooks.**

**Completed Steps:**

*   ✅ **`[Feature] Setup Project Structure & Cargo Workspace`**: Established the initial Rust project structure using a Cargo workspace (`khora_engine_core`, `sandbox`).
*   ✅ **`[Feature] Implement Core Math Library (Vec3, Mat4, Quat)`**: Implemented foundational 3D math types (`Vec3`, `Vec4`, `Mat4`, `Quat`) within `khora_engine_core`, designed with Data-Oriented principles in mind.

**Current Focus / Next Steps:**

Work is now beginning on defining the fundamental communication pathways and interfaces within the engine, keeping the long-term SAA goals in mind:

*   ➡️ **`[Feature] Design Core Engine Interfaces & Message Passing (Thinking about ISAs & DCC)`**: Defining initial interfaces between major future subsystems and considering a communication system (e.g., message bus) compatible with the idea of Agents (ISAs) communicating with a core (DCC).
*   **(Upcoming)** `[Feature] Implement Basic Logging & Event System`
*   **(Upcoming)** `[Feature] Implement Foundational Performance Monitoring Hooks (CPU Timers)`
*   **(Upcoming)** `[Feature] Implement Basic Memory Allocation Tracking`
*   **(Upcoming)** `[Feature] Choose and Integrate Windowing Library (e.g., winit)`
*   **(Upcoming)** `[Feature] Implement Basic Input System`
*   **(Upcoming)** `[Feature] Create Main Loop Structure`
*   **(Upcoming)** `[Task] Display Empty Window & Basic Stats (FPS, Mem)`

**Note:** This is a highly ambitious, long-term research and development project. The roadmap outlined ([link to roadmap if available, otherwise omit]) is extensive. Expect significant evolution, changes, and potential refactoring as development progresses towards the SAA goal.

## Getting Started

Currently, the project contains the basic structure and core math utilities.

```bash
# Clone the repository (replace with your actual URL)
git clone https://github.com/eraflo/KhoraEngine.git
cd KhoraEngine

# Check code and run tests (includes math library tests)
cargo check --workspace
cargo test --workspace

# Build the project
cargo build --workspace

# Run the sandbox (it still doesn't do much visually)
cargo run --bin sandbox