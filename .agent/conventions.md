# Khora Engine — Conventions

Code style and project conventions. Pair with [`rules.md`](./rules.md).

- Document — Khora Engine Conventions v1.0
- Status — Active
- Date — May 2026

---

## Contents

1. Naming
2. Code patterns
3. File layout
4. Components
5. Logging
6. Errors
7. Math
8. Git
9. Documentation

---

## 01 — Naming

| Element | Convention | Example |
|---|---|---|
| Crates | `khora-{name}` (kebab-case) | `khora-core`, `khora-lanes` |
| Modules | `snake_case` | `render_lane`, `service_registry` |
| Types and traits | `PascalCase` | `LaneContext`, `WgpuDevice` |
| Functions and methods | `snake_case` | `begin_frame`, `with_capacity` |
| Constants | `SCREAMING_SNAKE_CASE` | `MAX_LIGHTS`, `DEFAULT_BUDGET` |
| GPU resources | `…Id` suffix | `TextureId`, `BufferId`, `PipelineId` |
| Lanes | `…Lane` suffix | `LitForwardLane`, `ShadowPassLane` |
| Agents | `…Agent` suffix | `RenderAgent`, `UiAgent` |
| Services | `…Service` suffix | `AssetService`, `TelemetryService` |
| Flows | `…Flow` suffix | `RenderFlow`, `PhysicsFlow`, `ShadowFlow` |
| Flow Views | `…View` suffix | `RenderWorld`, `PhysicsView`, `AudioView` |
| Asset decoders | `…Decoder` suffix | `TextureDecoder`, `GltfDecoder` |

## 02 — Code patterns

- `Arc<dyn Trait>` for cross-crate shared references.
- `Arc<Mutex<T>>` for shared mutable state (e.g., `WgpuGraphicsContext`).
- `Result<T, SpecificError>` — never `unwrap()` on GPU/IO operations.
- Prefer trait objects over generic monomorphization for plugin-extensible surfaces.
- Use `#[non_exhaustive]` on public enums and structs that are likely to grow.
- Newtype wrappers around primitive IDs (`pub struct TextureId(u64)`) — no bare integers in public APIs.

## 03 — File layout

- One primary type per file (`device.rs` → `WgpuDevice`).
- Tests live in `#[cfg(test)] mod tests` at the bottom of each module file.
- Integration tests in `crates/{name}/tests/`.
- Benchmarks in `crates/{name}/benches/`.
- Backend implementations grouped per backend folder under `khora-infra/src/`:
  - `graphics/wgpu/`
  - `physics/rapier/`
  - `audio/cpal/`
  - `ui/taffy/`
- Shaders under `crates/khora-lanes/src/render_lane/shaders/`, one `.wgsl` file per pass.
- **Invariant `DataSystem`s**: one file per system in `crates/khora-data/src/ecs/systems/`. Discovered by the engine via `inventory::submit!{ DataSystemRegistration { ... } }`. Zero wiring elsewhere.
- **Domain `Flow`s**: one file per flow in `crates/khora-data/src/flow/`. Same `inventory` pattern (use the `register_flow!` macro). The Scheduler's Substrate Pass runs them automatically.
- **Asset decoders**: one file per decoder in `crates/khora-io/src/asset/decoders/`. Registered with the `AssetService` at startup; the file should drop the `…Lane` legacy suffix.

### 03.1 — Adding a new domain

1. Author the **`Flow`** in `khora-data/src/flow/<domain>.rs` — implement `Flow`, define a typed `View`, optionally use `Flow::adapt` for AGDF mutations. Register with `register_flow!(MyFlow)`.
2. Author the **`Agent`** in `khora-agents/src/<domain>_agent/` — strict strategist, no per-frame state, invokes its lane with `(bus, deck, budget)` from `EngineContext`.
3. Author the **`Lane`s** in `khora-lanes/src/<domain>_lane/` — read the `View` from `LaneContext::bus`, write outputs into `LaneContext::deck`. Never call `world.query*`.
4. The engine drains the relevant typed slot of the `OutputDeck` at the I/O boundary if the domain produces side-effects (GPU submit, audio buffer flush, etc.).

## 04 — Components

All ECS components use `#[derive(Component)]`. The macro generates a `SerializableX` mirror struct plus `From` conversions in both directions.

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

- `#[component(skip)]` on fields that must not be serialized (GPU handles, runtime caches).
- `#[component(no_serializable)]` on components with manual mirrors (unit structs, trait objects).
- Register every component via `inventory::submit!` in `crates/khora-data/src/ecs/components/registrations.rs`. Use the `register_components!` macro for batch registration.

## 05 — Logging

- `log::info!` for lifecycle events (startup, agent registration, swapchain resize).
- `log::warn!` for recoverable anomalies (asset retry, budget shortfall).
- `log::error!` for unrecoverable errors before bubbling up a `Result`.
- `log::debug!` and `log::trace!` for hot-path diagnostics, gated behind log level.
- **Never** use `println!` or `eprintln!`. They bypass the log pipeline and will not appear in editor consoles or telemetry.

## 06 — Errors

- Each subsystem owns its error enum: `LaneError`, `AssetError`, `PhysicsError`, etc.
- Errors derive `thiserror::Error`. Variants carry context (paths, IDs, expected state).
- Use `?` for propagation. Use `map_err` only when adding context.
- Boundaries (file I/O, GPU calls, FFI) are the only places where errors originate. Internal calls trust their callers.

## 07 — Math

- Right-handed coordinate system, column-major matrices, **Y-up**.
- All math through `khora_core::math`. Extend the module when needed; never bypass it with raw `glam`.
- Vec types: `Vec2`, `Vec3`, `Vec4`. Matrices: `Mat3`, `Mat4`. Rotation: `Quat`. Bounds: `Aabb`. Color: `LinearRgba`.
- For SIMD-friendly hot paths, use `Vec4`-aligned layouts.
- Document non-trivial derivations in comments, with the source paper or formula reference.

## 08 — Git

- Development happens on the `dev` branch. Stable releases live on `main`.
- Pre-commit: `cargo xtask all` (fmt + clippy + test + doc).
- CI: GitHub Actions in `.github/workflows/rust.yml`.
- Commit messages: imperative mood, prefix optional (`feat:`, `fix:`, `refacto:`, `docs:`).
- Never push without explicit user permission.

## 09 — Documentation

- mdBook source lives in `docs/src/*.md`. Build with `mdbook build docs/`.
- Custom Ayu-dark theme in `docs/theme/custom.css`.
- Public APIs require rustdoc. `# Examples` blocks compile via `cargo test --doc`.
- Use intra-doc links (`[Type]`) instead of plain text references.
- Keep mdBook chapters in sync with the codebase. If you change a public API, update the relevant chapter in the same PR.

---

*End of conventions.*
