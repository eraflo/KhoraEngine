# 5. Project and Crate Structure

The Khora project is organized as a Cargo workspace to enforce modularity, enable efficient compilation, and reflect our CLAD architecture. This document provides a high-level overview of the repository's layout.

### Top-Level Directory Structure

*   `.github/`: Contains GitHub-specific configurations like CI workflows and issue templates.
*   `crates/`: The heart of the engine. Contains all the core `khora-*` source code, organized into modular crates.
*   `docs/`: Contains all project documentation, including the source for this book.
*   `examples/`: Engine usage examples and testbeds, with `sandbox` being our primary test application demonstrating `khora-sdk` usage without internal `khora-core` dependencies.
*   `resources/`: Runtime configuration files, such as default profiles for the DCC.
*   `xtask/`: A dedicated crate for build automation and scripting tasks (`cargo-xtask`).
*   `Cargo.toml`: The root workspace definition, specifying members and compilation profiles.

### Detailed Crate Layout

The following is a representative layout of the `crates/` directory, illustrating the internal structure of our CLAD implementation.

```
crates/
├── khora-core/      # FOUNDATIONAL: Traits, core types, interface contracts.
│   └── src/
│       ├── lane/            # Lane trait, LaneContext, LaneRegistry, Slot/Ref, context_keys
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
├── khora-lanes/     # [L]ANES: Hot-path execution pipelines (all implement Lane).
│   └── src/
│       ├── asset_lane/      # PackLoadingLane (VFS pack file streaming)
│       ├── audio_lane/
│       │   └── mixing/      # SpatialMixingLane (3D audio mixing)
│       ├── ecs_lane/        # CompactionLane (archetype memory defragmentation)
│       ├── physics_lane/    # StandardPhysicsLane, VerletPhysicsLane
│       ├── render_lane/
│       │   ├── shaders/     # WGSL: lit_forward.wgsl, shadow_depth.wgsl, etc.
│       │   ├── simple_unlit_lane.rs
│       │   ├── lit_forward_lane.rs
│       │   ├── forward_plus_lane.rs
│       │   ├── shadow_pass_lane.rs
│       │   ├── extract_lane.rs
│       │   └── world.rs     # RenderWorld, ExtractedLight, ExtractedMesh
│       └── scene_lane/      # ArchetypeLane, SnapshotLane (serialization strategies)
│
├── khora-agents/    # [A]GENTS: Intelligent wrappers driving the Lanes.
│   └── src/
│       ├── render_agent/        # RenderAgent — GPU rendering ISA (GORNA ✅)
│       ├── physics_agent/       # PhysicsAgent — physics simulation ISA (GORNA ✅)
│       ├── audio_agent/         # AudioAgent — spatial audio mixing
│       ├── asset_agent/         # AssetAgent — async asset loading
│       ├── serialization_agent/ # SerializationAgent — scene persistence
│       └── ecs_agent/           # GarbageCollectorAgent — ECS maintenance
│
├── khora-telemetry/ # Central service for metrics and monitoring.
│   └── src/
│
├── khora-infra/     # Concrete implementations of external dependencies.
│   └── src/
│       ├── audio/
│       ├── graphics/    # wgpu backend (device, command encoding, pipelines)
│       ├── physics/
│       ├── platform/
│       └── telemetry/
│
├── khora-sdk/       # [USER-FACING] The stable, public-facing API for game developers.
│   └── src/         # No internal engine data structures are exposed; only traits and Vessels.
│
├── khora-editor/    # [TOOLING] The engine's editor GUI using egui.
│   └── src/
│
├── khora-plugins/   # [EXTENSIONS] Packaged strategies and extensions.
│   └── src/
│
└── khora-macros/    # Procedural macros for code generation (derive macros).
    └── src/
```