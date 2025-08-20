<p align="center">
  <img src="docs/assets/logos/khora_full_logo.png" alt="Khora Engine Logo" width="250">
</p>

<h1 align="center">Khora Engine</h1>
<p align="center">
    <a href="https://github.com/eraflo/KhoraEngine/actions/workflows/rust.yml"><img src="https://github.com/eraflo/KhoraEngine/actions/workflows/rust.yml/badge.svg" alt="Rust CI"/></a>
</p>
<p align="center">
    Khora is an experimental game engine being developed in Rust, with the goal of implementing a novel <strong>Symbiotic Adaptive Architecture (SAA)</strong>.
</p>

## Our Vision: A Symbiotic, Self-Optimizing Architecture

Khora is not a traditional game engine. It is a living system, context-aware, that continuously adapts to deliver the best possible experience. Its subsystems are not just gears in a machine, but **intelligent agents** that collaborate and negotiate for resources in real time.

This approach aims to solve the fundamental problems of modern engines: costly manual optimization, rigid pipelines, and the inability to adapt to growing hardware diversity.

### Key Principles

*   **Context Awareness**: The engine maintains a real-time model of the current situation (CPU/GPU load, scene complexity, performance goals).
*   **Resource Negotiation**: Subsystems (rendering, physics, AI) request resource budgets, and the engine allocates them dynamically to meet global objectives.
*   **Strategic Adaptation**: Each subsystem can switch its algorithms on the fly (e.g., from Forward to Deferred rendering) based on the context.
*   **Data-Oriented & Adaptive**: The organization of data itself can be optimized based on observed access patterns.

## Project Status

The project is under active development. The foundational architecture has been refactored into a modular, decoupled CLAD structure. The current focus is on implementing the core SAA logic and advanced features.

For a detailed overview of completed and upcoming tasks, please see our **[Roadmap & Issue Tracker](docs/05_roadmap_and_issues.md)**.

## Full Documentation

All project documentation, from the high-level vision to the technical architecture details, can be found in the [`/docs`](/docs) directory.

*   **[Documentation Index](docs/README.md)** - The starting point to explore everything.
*   **[Core Concepts (SAA)](docs/02_core_concepts.md)** - Understand the philosophy behind Khora.
*   **[Technical Architecture (CLAD)](docs/03_technical_architecture.md)** - A deep dive into the Rust implementation.

## Getting Started

```bash
# Clone the repository
git clone https://github.com/eraflo/KhoraEngine.git
cd KhoraEngine

# Run tests for the entire workspace
cargo test --workspace

# Check if your changes are good to go in prod
cargo xtask all

# Run the sandbox application
cargo run -p sandbox```
```

## Community & Contributing

Khora is an ambitious open-source project and we welcome all contributions.

*   Please read our [**Code of Conduct**](CODE_OF_CONDUCT.md) and [**Contributing Guidelines**](CONTRIBUTING.md).
*   For general discussions, ideas, and questions, join us on [**GitHub Discussions**](https://github.com/eraflo/KhoraEngine/discussions).
*   For bug reports or feature requests, please open an [**Issue**](https://github.com/eraflo/KhoraEngine/issues).

## License

Khora Engine is licensed under the [Apache License 2.0](LICENSE).