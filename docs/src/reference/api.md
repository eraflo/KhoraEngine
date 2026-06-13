# API reference

The generated rustdoc is the exhaustive reference for every public crate in the
workspace. The book covers concepts and curated maps; the rustdoc covers every
type, trait, function, and method.

The published reference is built from `main` on every release and lives at
[eraflo.github.io/KhoraEngine/api](https://eraflo.github.io/KhoraEngine/api/index.html).

## Public SDK

`khora-sdk` is the only crate game developers should depend on. Everything else
is implementation detail. See the [SDK surface](./sdk.md) page for a curated map.

| Entry | rustdoc |
|---|---|
| Crate root | [`khora_sdk`](https://eraflo.github.io/KhoraEngine/api/khora_sdk/index.html) |
| Prelude | [`khora_sdk::prelude`](https://eraflo.github.io/KhoraEngine/api/khora_sdk/prelude/index.html) |
| Engine type | [`EngineCore`](https://eraflo.github.io/KhoraEngine/api/khora_sdk/struct.EngineCore.html) |
| ECS facade | [`GameWorld`](https://eraflo.github.io/KhoraEngine/api/khora_sdk/struct.GameWorld.html) |
| App lifecycle trait | [`EngineApp`](https://eraflo.github.io/KhoraEngine/api/khora_sdk/trait.EngineApp.html) |
| Agent registration trait | [`AgentProvider`](https://eraflo.github.io/KhoraEngine/api/khora_sdk/trait.AgentProvider.html) |
| Custom phase trait | [`PhaseProvider`](https://eraflo.github.io/KhoraEngine/api/khora_sdk/trait.PhaseProvider.html) |
| Spawn builder | [`Vessel`](https://eraflo.github.io/KhoraEngine/api/khora_sdk/struct.Vessel.html) |
| Bootstrap entry | [`run_winit`](https://eraflo.github.io/KhoraEngine/api/khora_sdk/fn.run_winit.html) |
| Window settings | [`WindowConfig`](https://eraflo.github.io/KhoraEngine/api/khora_sdk/struct.WindowConfig.html) |

## Internal crates

Visible for engine contributors. Game code should not depend on these directly —
their surface is re-exported through `khora-sdk` where game code needs it. See the
[crate map](./crates.md) for the dependency layering.

| Crate | rustdoc |
|---|---|
| `khora_core` | [Trait floor — math, GORNA types, traits, runtime containers](https://eraflo.github.io/KhoraEngine/api/khora_core/index.html) |
| `khora_data` | [CRPECS ECS, component storage, Flows, scene strategies](https://eraflo.github.io/KhoraEngine/api/khora_data/index.html) |
| `khora_io` | [VFS, asset service, serialization service, pack builder](https://eraflo.github.io/KhoraEngine/api/khora_io/index.html) |
| `khora_lanes` | [Render, physics, audio, asset, scene, UI lanes](https://eraflo.github.io/KhoraEngine/api/khora_lanes/index.html) |
| `khora_agents` | [The strategist agents + `PhysicsQueryService`](https://eraflo.github.io/KhoraEngine/api/khora_agents/index.html) |
| `khora_control` | [DCC, scheduler, GORNA arbitration, plugin](https://eraflo.github.io/KhoraEngine/api/khora_control/index.html) |
| `khora_infra` | [Default backends — wgpu, Rapier, CPAL, Taffy, winit](https://eraflo.github.io/KhoraEngine/api/khora_infra/index.html) |
| `khora_telemetry` | [Telemetry service, metrics, monitors](https://eraflo.github.io/KhoraEngine/api/khora_telemetry/index.html) |
| `khora_macros` | [`#[derive(Component)]` proc macro](https://eraflo.github.io/KhoraEngine/api/khora_macros/index.html) |
| `khora_plugins` | [Plugin loading and registration](https://eraflo.github.io/KhoraEngine/api/khora_plugins/index.html) |
| `khora_editor` | [Editor application](https://eraflo.github.io/KhoraEngine/api/khora_editor/index.html) |

## Generating locally

To build the same rustdoc on your machine:

```bash
cargo doc --workspace --no-deps --open
```

The output lands under `target/doc/`. The combined documentation site (this book
plus rustdoc mounted under `/api`) is assembled by `.github/workflows/docs.yml`
on every push to `main`; there is no single local command that mirrors the full
combined site.

---

*The API reference is the contract. The book is the rationale — start with
[Concepts](../concepts/index.md).*
