# API reference

Generated rustdoc for every public crate in the workspace. The book covers concepts; the API reference covers every type, trait, function, and method.

- Document — Khora API Reference Index v1.0
- Status — Generated
- Date — May 2026

---

## Contents

1. Public SDK
2. Internal crates
3. Generating locally

---

## 01 — Public SDK

The only crate game developers should depend on. Everything else is implementation detail.

| Module | Documentation |
|---|---|
| `khora_sdk` | [Crate root](https://eraflo.github.io/KhoraEngine/api/khora_sdk/index.html) |
| `khora_sdk::prelude` | [Prelude](https://eraflo.github.io/KhoraEngine/api/khora_sdk/prelude/index.html) |
| `khora_sdk::EngineCore` | [Engine type](https://eraflo.github.io/KhoraEngine/api/khora_sdk/struct.EngineCore.html) |
| `khora_sdk::GameWorld` | [ECS facade](https://eraflo.github.io/KhoraEngine/api/khora_sdk/struct.GameWorld.html) |
| `khora_sdk::EngineApp` | [App lifecycle trait](https://eraflo.github.io/KhoraEngine/api/khora_sdk/trait.EngineApp.html) |
| `khora_sdk::AgentProvider` | [Agent registration trait](https://eraflo.github.io/KhoraEngine/api/khora_sdk/trait.AgentProvider.html) |
| `khora_sdk::PhaseProvider` | [Custom phase trait](https://eraflo.github.io/KhoraEngine/api/khora_sdk/trait.PhaseProvider.html) |
| `khora_sdk::Vessel` | [Spawn builder](https://eraflo.github.io/KhoraEngine/api/khora_sdk/struct.Vessel.html) |
| `khora_sdk::run_winit` | [Bootstrap entry](https://eraflo.github.io/KhoraEngine/api/khora_sdk/fn.run_winit.html) |
| `khora_sdk::WindowConfig` | [Window settings](https://eraflo.github.io/KhoraEngine/api/khora_sdk/struct.WindowConfig.html) |

## 02 — Internal crates

These are visible for engine contributors. Game code should not depend on them directly.

| Crate | Documentation |
|---|---|
| `khora_core` | [Trait floor — math, GORNA types, traits](https://eraflo.github.io/KhoraEngine/api/khora_core/index.html) |
| `khora_data` | [CRPECS ECS, allocators, components](https://eraflo.github.io/KhoraEngine/api/khora_data/index.html) |
| `khora_io` | [VFS, asset service, serialization](https://eraflo.github.io/KhoraEngine/api/khora_io/index.html) |
| `khora_lanes` | [Render, physics, audio, asset, scene lanes](https://eraflo.github.io/KhoraEngine/api/khora_lanes/index.html) |
| `khora_agents` | [The five agents + PhysicsQueryService](https://eraflo.github.io/KhoraEngine/api/khora_agents/index.html) |
| `khora_control` | [DCC, scheduler, GORNA arbitration, plugin](https://eraflo.github.io/KhoraEngine/api/khora_control/index.html) |
| `khora_infra` | [Default backends — wgpu, Rapier, CPAL, Taffy](https://eraflo.github.io/KhoraEngine/api/khora_infra/index.html) |
| `khora_telemetry` | [Telemetry service, metrics, monitors](https://eraflo.github.io/KhoraEngine/api/khora_telemetry/index.html) |
| `khora_macros` | [`#[derive(Component)]` proc macro](https://eraflo.github.io/KhoraEngine/api/khora_macros/index.html) |
| `khora_plugins` | [Plugin loading and registration](https://eraflo.github.io/KhoraEngine/api/khora_plugins/index.html) |
| `khora_editor` | [Editor application](https://eraflo.github.io/KhoraEngine/api/khora_editor/index.html) |

## 03 — Generating locally

The published reference at [eraflo.github.io/KhoraEngine/api](https://eraflo.github.io/KhoraEngine/api/index.html) is built from `main` on every release. To generate the same docs locally:

```bash
cargo doc --workspace --no-deps --open
```

The output lands under `target/doc/`. To bundle the workspace docs alongside the mdBook output (the way CI does it), the project's `xtask` provides a single command:

```bash
cargo xtask docs
```

This builds the mdBook into `docs/book/` and the rustdoc into `docs/book/api/`, ready to publish as one site.

---

*The API reference is the contract. The book is the rationale.*
