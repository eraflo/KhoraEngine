# 5. Project and Crate Structure

The Khora project is organized as a Cargo workspace to enforce modularity, enable efficient compilation, and reflect our CLAD architecture. This document provides a high-level overview of the repository's layout.

### Top-Level Directory Structure

*   `.github/`: Contains GitHub-specific configurations like CI workflows and issue templates.
*   `crates/`: The heart of the engine. Contains all the core `khora-*` source code, organized into modular crates.
*   `docs/`: Contains all project documentation, including the source for this book.
*   `examples/`: Engine usage examples and testbeds, with `sandbox` being our primary test application.
*   `resources/`: Runtime configuration files, such as default profiles for the DCC.
*   `xtask/`: A dedicated crate for build automation and scripting tasks (`cargo-xtask`).
*   `Cargo.toml`: The root workspace definition, specifying members and compilation profiles.

### Detailed Crate Layout

The following is a representative layout of the `crates/` directory, illustrating the internal structure of our CLAD implementation.

```
crates/
├── khora-core/      # FOUNDATIONAL: Traits, core types, interface contracts.
│   └── src/
│       ├── math/
│       ├── platform/
│       ├── renderer/
│       └── ...
│
├── khora-control/   # [C]ONTROL: DCC and GORNA implementation.
│   └── src/
│
├── khora-data/      # [D]ATA: CRPECS, resources, and other data containers.
│   └── src/
│       ├── ecs/
│       └── ...
│
├── khora-lanes/     # [L]ANES: Hot-path execution pipelines (systems).
│   └── src/
│       ├── asset_lane/
│       ├── audio_lane/
│       ├── ecs_lane/
│       ├── physics_lane/
│       ├── render_lane/
│       └── scene_lane/
│
├── khora-agents/    # [A]GENTS: Intelligent wrappers driving the Lanes.
│   └── src/
│       ├── render_agent/
│       └── ...
│
├── khora-telemetry/ # Central service for metrics and monitoring.
│   └── src/
│
├── khora-infra/     # Concrete implementations of external dependencies.
│   └── src/
│       ├── audio/
│       ├── graphics/
│       ├── physics/
│       ├── platform/
│       └── telemetry/
│
├── khora-sdk/       # The stable, public-facing API for game developers.
│   └── src/
│
├── khora-editor/    # [FUTURE] The engine's editor GUI (placeholder).
│   └── src/
│
├── khora-plugins/   # [FUTURE] Packaged strategies and extensions (planned).
│   └── src/
│
└── khora-macros/    # Procedural macros for code generation (derive macros).
    └── src/
```