# Khora Engine — Conventions (engine profile)

Code style and project conventions. Pairs with [`RULES.md`](./RULES.md).

- Document — Khora Engine Conventions v2.0
- Status — Active

---

## 1 — Naming

| Element | Convention | Example |
|---|---|---|
| Crates | `khora-{name}` (kebab-case) | `khora-core`, `khora-lanes` |
| Modules | `snake_case` | `render_lane`, `service_registry` |
| Types & traits | `PascalCase` | `LaneContext`, `WgpuDevice` |
| Functions & methods | `snake_case` | `begin_frame`, `with_capacity` |
| Constants | `SCREAMING_SNAKE_CASE` | `MAX_LIGHTS`, `DEFAULT_BUDGET` |
| GPU resources | `…Id` suffix | `TextureId`, `BufferId`, `PipelineId` |
| Lanes | `…Lane` suffix | `LitForwardLane`, `ShadowPassLane` |
| Agents | `…Agent` suffix | `RenderAgent`, `UiAgent` |
| Services | `…Service` suffix | `AssetService`, `TelemetryService` |
| Flows | `…Flow` suffix | `RenderFlow`, `PhysicsFlow`, `ShadowFlow` |
| Flow Views | `…View` / `…World` suffix | `RenderWorld`, `PhysicsView`, `AudioView` |
| Asset decoders | `…Decoder` suffix | `SymphoniaDecoder`, `GltfDecoder` |

## 2 — Code patterns

- `Arc<dyn Trait>` for cross-crate shared references; `Arc<Mutex<T>>` for shared mutable state (e.g. `WgpuGraphicsContext`).
- `Result<T, SpecificError>` — never `unwrap()` on GPU/IO operations.
- Prefer trait objects over generic monomorphization for plugin-extensible surfaces.
- `#[non_exhaustive]` on public enums/structs likely to grow.
- Newtype wrappers around primitive IDs (`pub struct TextureId(u64)`) — no bare integers in public APIs.

## 3 — File layout

- One primary type per file (`device.rs` → `WgpuDevice`).
- Unit tests in `#[cfg(test)] mod tests` at the bottom of each module file.
- Integration tests in `crates/{name}/tests/`; benchmarks in `crates/{name}/benches/`.
- Backend implementations grouped per backend folder under `khora-infra/src/`: `graphics/wgpu/`, `physics/rapier/`, `audio/cpal/`, `ui/taffy/`.
- Shaders under `crates/khora-lanes/src/render_lane/shaders/` — `pipelines/` (entry points) and `lib/` (reusable modules). One pass per pipeline file; no `.wgsl` at the shaders root.
- **Invariant `DataSystem`s**: one file per system in `crates/khora-data/src/ecs/systems/`. Discovered via `inventory::submit!{ DataSystemRegistration { … } }`. Zero wiring elsewhere.
- **Domain `Flow`s**: one file per flow in `crates/khora-data/src/flow/`. Register with `register_flow!(MyFlow)`. The Scheduler's Substrate Pass runs them automatically.
- **Asset decoders**: one file per decoder in `crates/khora-io/src/asset/decoders/`. Registered with the `AssetService`; the `…Lane` legacy suffix is dropped.

### 3.1 — Adding a new domain

1. **`Flow`** in `khora-data/src/flow/<domain>.rs` — implement `Flow` (read-only `select → project`), define a typed `View`, register with `register_flow!`. A Flow never mutates the World; per-frame maintenance goes in a `DataSystem`.
2. **`Agent`** in `khora-agents/src/<domain>_agent/` — strict strategist, no per-frame state, invokes its lane with `(bus, deck, budget)` from `EngineContext`.
3. **`Lane`s** in `khora-lanes/src/<domain>_lane/` — read the `View` from `LaneContext::bus`, write outputs into `LaneContext::deck`. Never call `world.query*`.
4. The engine drains the relevant typed slot of the `OutputDeck` at the I/O boundary for side-effects (GPU submit, audio flush…).

## 4 — Components

`#[derive(Component)]` on all ECS components; the macro generates a `SerializableX` mirror + `From` conversions both ways.

```rust
#[derive(Component)]
pub struct Light {
    pub kind: LightKind,
    pub color: LinearRgba,
    pub intensity: f32,
    #[component(skip)]                  // GPU handle, runtime-only
    pub gpu_resource: Option<TextureId>,
}
```

- `#[component(skip)]` for fields that must not serialize (GPU handles, runtime caches).
- `#[component(no_serializable)]` for components with manual mirrors (unit structs, trait objects).
- The derive self-registers via `inventory`. Batch registrations live in `crates/khora-data/src/ecs/components/registrations.rs` (`register_components!`). Only generics and hand-written impls stay explicit in `World::new`.

## 5 — Logging

- `log::info!` — lifecycle events (startup, agent registration, swapchain resize).
- `log::warn!` — recoverable anomalies (asset retry, budget shortfall).
- `log::error!` — unrecoverable errors before bubbling a `Result`.
- `log::debug!` / `log::trace!` — hot-path diagnostics, gated by level.
- **Never** `println!` / `eprintln!` — they bypass the log pipeline and editor consoles.

## 6 — Errors

- Each subsystem owns its error enum: `LaneError`, `AssetError`, `PhysicsError`, …
- Errors derive `thiserror::Error`; variants carry context (paths, IDs, expected state).
- `?` for propagation; `map_err` only to add context. Errors originate at boundaries (file I/O, GPU, FFI); internal calls trust their callers.

## 7 — Math

- Right-handed, column-major matrices, **Y-up**.
- All math through `khora_core::math` — never raw `glam`. Extend the module when needed.
- Vectors `Vec2/3/4`, matrices `Mat3/4`, rotation `Quaternion`, bounds `Aabb`, color `LinearRgba`.
- For SIMD-friendly hot paths use `Vec4`-aligned layouts; explicit batch kernels live in `khora-core/src/math/simd.rs` (`TrsBatchSoa`, `wide::f32x8`).
- Document non-trivial derivations with the source paper/formula.

## 8 — Git

- Develop on `dev`; stable releases on `main`.
- Pre-commit: `cargo xtask all` (fmt + clippy + test + doc). A `secret-scan` pre-commit hook (installed by the AI installer) blocks commits containing secrets.
- CI: GitHub Actions in `.github/workflows/`.
- Commit messages: imperative mood, optional prefix (`feat:`, `fix:`, `refacto:`, `docs:`).
- Never push without explicit user permission.

## 9 — Documentation

- mdBook source in `docs/src/*.md` (`mdbook build docs/`). Custom Ayu-dark theme in `docs/theme/custom.css`.
- Document in existing files in place — **no separate ADR section/files** for this project.
- Public APIs require rustdoc; `# Examples` blocks compile via `cargo test --doc`. Use intra-doc links (`[Type]`).
- Keep mdBook chapters in sync with the code: change a public API → update the chapter in the same change.
- For any **design / UI-UX** task (editor visuals, docs visual language, game UI), use **`/impeccable`**.

## 10 — Render bind-group budget

Every render lane uses **exactly four bind groups**, the four stable inputs of a draw:

| Group | Domain | Contents |
|---|---|---|
| 0 | **Frame** | camera (view-projection, position) |
| 1 | **Object** | model + normal matrix (per-draw) |
| 2 | **Material** | base color, emissive, … |
| 3 | **Lighting** | *every* lighting input |

Group 3 is the **whole lighting domain** — direct lights, per-tile culling results, shadow atlases and
matrices. Shadow atlas / sampler / cube bindings are fixed at indices **1 / 2 / 3** (the shared
`khora::shadow::bindings` WGSL contract, mirrored by `khora_core::renderer::api::shadow::bindings`); a
lane packs its lighting buffers into the remaining indices (0, 4, 5, …).

**Rule** — a new lighting feature (clustered lighting, GI probes, a new shadow technique) adds
**bindings to group 3**, never a 5th group. Why: 4 groups is the wgpu universal baseline
(`Limits::default().max_bind_groups == 4`) — works on every backend with no capability negotiation;
bindings *within* a group are effectively unbounded (~1000). Compute pipelines (e.g. Forward+ light
culling) own their own layout and are exempt.

---

*End of conventions.*
