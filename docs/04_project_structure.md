# 04 - Project and Crate Structure

The Khora project is organized as a Cargo workspace to ensure modularity and efficient compilation. This is the complete layout of the project, showing how code, documentation, and resources are organized.

  

khora/
├── .github/
│ └── workflows/
│ └── rust.yml # Continuous Integration (CI) configuration.
├── Cargo.toml # Workspace definition and compilation profiles.
├── rust-toolchain.toml # Specifies the Rust toolchain version.
├── LICENSE
├── CODE_OF_CONDUCT.md
├── CONTRIBUTING.md
│
├── assets/ # Default assets shipped with the engine (shaders, textures).
│ ├── shaders/
│ └── textures/
│
├── resources/ # Configuration files loaded at runtime.
│ └── config/
│ └── default_dcc_profile.json
│
├── xtask/ # Build automation and scripting tasks (cargo-xtask).
│ ├── Cargo.toml
│ └── src/
│ └── main.rs
│
├── docs/ # <-- All project documentation.
│ ├── README.md # Documentation index.
│ ├── 01_project_presentation.md
│ ├── 02_core_concepts.md
│ ├── 03_technical_architecture.md
│ ├── 04_project_structure.md
│ └── 05_roadmap_and_issues.md
│ └── assets/
│ └── logos/
│ └── khora_full_logo.png
│
├── examples/ # Engine usage examples and testbeds.
│ └── sandbox/
│ ├── Cargo.toml
│ └── src/
│ └── main.rs # The main binary for testing and demos.
│
├── crates/ # The engine's core, organized into modular crates.
│ │
│ ├── khora-core/ # FOUNDATIONAL CRATE: Traits, core types, interface contracts.
│ │ ├── Cargo.toml
│ │ └── src/
│ │ └── lib.rs
│ │
│ ├── khora-control/ # [C]ONTROL: DCC and GORNA implementation.
│ │ ├── Cargo.toml
│ │ └── src/
│ │ └── lib.rs
│ │
│ ├── khora-data/ # [D]ATA: Data layouts, allocators, streaming.
│ │ ├── Cargo.toml
│ │ └── src/
│ │ └── lib.rs
│ │
│ ├── khora-lanes/ # [L]ANES: Hot-path execution pipelines.
│ │ ├── Cargo.toml
│ │ └── src/
│ │ ├── lib.rs
│ │ ├── render_lane/
│ │ ├── physics_lane/
│ │ └── audio_lane/
│ │
│ ├── khora-agents/ # [A]GENTS: Intelligent wrappers driving the Lanes.
│ │ ├── Cargo.toml
│ │ └── src/
│ │ ├── lib.rs
│ │ ├── render_agent/
│ │ ├── physics_agent/
│ │ └── asset_agent/
│ │
│ ├── khora-infra/ # Concrete implementations of external dependencies.
│ │ ├── Cargo.toml
│ │ └── src/
│ │ ├── lib.rs
│ │ ├── graphics_wgpu/
│ │ └── platform_winit/
│ │
│ ├── khora-editor/ # The engine's editor GUI.
│ │ ├── Cargo.toml
│ │ └── src/
│ │ └── main.rs
│ │
│ ├── khora-plugins/ # Packaged strategies and extensions.
│ │ ├── Cargo.toml
│ │ └── src/
│ │ └── lib.rs
│ │
│ └── khora-sdk/ # The stable, public-facing API for game developers.
│ ├── Cargo.toml
│ └── src/
│ └── lib.rs
│
└── tests/ # Integration and scenario tests.
├── integration/
└── scenarios/