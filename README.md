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

**This project is currently at the very beginning of its journey.**

We are executing the **first step** of **Milestone 1: Core Foundation & Context Hooks**:

*   **`[Feature] Setup Project Structure & Cargo Workspace`**: Establishing the initial Rust project structure using a Cargo workspace to foster modularity from the outset. The initial crates are:
    *   `khora_engine_core`: The library crate that will contain the engine's core logic.
    *   `sandbox`: A binary crate to test and demonstrate the engine's features.

**Next immediate steps will involve:** Implementing core math primitives, basic logging/event systems, windowing integration, and foundational performance/memory monitoring hooks – all crucial groundwork for the SAA.

**Note:** This is a highly ambitious, long-term research and development project. The roadmap outlined ([link to roadmap if available, otherwise omit]) is extensive. Expect significant evolution, changes, and potential refactoring as development progresses towards the SAA goal.

## Getting Started

Currently, the project only contains the basic structure.

```bash
# Clone the repository (replace with your actual URL)
git clone https://github.com/eraflo/KhoraEngine.git
cd KhoraEngine

# Build the project (this will build both engine_core and sandbox)
cargo build

# Run the sandbox (it won't do much yet)
cargo run --bin sandbox