# Khora Engine — Conventions (engine profile)

Code style and project conventions. Pairs with [`RULES.md`](./RULES.md).

- Document — Khora Engine Conventions v2.0
- Status — Active

---

## 1 — Naming

| Element | Convention | Example |
|---|---|---|
| Crates | `khora-{name}` (kebab-case) | `khora-core`, `khora-lanes` |
| Modules | `snake_case` | `spatial_mixing`, `service_registry` |
| Types & traits | `PascalCase` | `LaneBus`, `WgpuDevice` |
| Functions & methods | `snake_case` | `begin_frame`, `with_capacity` |
| Constants | `SCREAMING_SNAKE_CASE` | `MAX_LIGHTS`, `DEFAULT_BUDGET` |
| GPU resources | `…Id` suffix | `TextureId`, `BufferId`, `PipelineId` |
| Lanes | `…Lane` suffix | `LitForwardLane`, `ShadowsLane` |
| Lane families | `…Family` suffix | `RenderFamily`, `ShadowsFamily` (no agent type: every family's agent is `LaneAgent`) |
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

- One primary type per file (`mat4.rs` → `Mat4`).
- Unit tests in `#[cfg(test)] mod tests` at the bottom of each module file.
- Integration tests in `crates/{name}/tests/`; benchmarks in `crates/{name}/benches/`.
- Backend implementations grouped per backend folder under `khora-infra/src/`: `graphics/wgpu/`, `physics/rapier/`, `audio/cpal/`, `ui/taffy/`.
- Shaders under `crates/khora-infra/src/graphics/shader/shaders/` — `pipelines/` (entry points) and `lib/` (reusable modules). One pass per pipeline file; no `.wgsl` at the shaders root.
- **Invariant `DataSystem`s**: one file per system in `crates/khora-data/src/ecs/systems/`. Discovered via `inventory::submit!{ DataSystemRegistration { … } }`. Zero wiring elsewhere. The function is `fn(&mut World, &Runtime, &mut OutputDeck, SystemContext)`; `ctx.since` is the tick of its previous run, read with `Changed<T>`, and the components it watches go in `observes`. `role` (no default) is `Role::Upkeep` (a data invariant, every frame) or `Role::Serves(family)` (skipped while that family is off, `khora_data::ecs::Role`); `ctx.resumed` tells its first run on a world, and its first run after a skip. A `Flow` declares the same as `const ROLE`, and its `select` gets `FlowContext { resumed }`. The boot refuses a `Serves` naming no registered family (`check_roles`).
- **Domain `Flow`s**: one file per flow in `crates/khora-data/src/flow/`. Register with `register_flow!(MyFlow)`. The Scheduler's Substrate Pass runs them automatically.
- **Asset decoders**: one file per decoder in `crates/khora-io/src/asset/decoders/`. Registered with the `AssetService`; the `…Lane` legacy suffix is dropped.

### 3.1 — Adding a new domain

1. **`Flow`** in `khora-data/src/flow/<domain>.rs` — implement `Flow` (read-only `select → project`), define a typed `View`, register with `register_flow!`. A Flow never mutates the World; per-frame maintenance goes in a `DataSystem`.
2. **`Lane`s and their `LaneFamily`** in `khora-lanes/src/<family>/` (`family.rs`, its lanes, an input type shared by several lanes in a file of its own) — each lane implements `khora_core::lane::Lane` (`levels`, `prepare`, `cost`, `record`, `shutdown`); it reads typed inputs (`#[derive(LaneInputs)]`: `#[bus]` the `View`, `#[deck]`, `#[resource]`, `#[locked]`, …) and writes the family's typed outputs (`#[derive(LaneOutputs)]`: `#[pass]`, `#[deck]`, `#[staged]`). Never call `world.query*`.
3. **One registration** in `khora-lanes/src/<family>/registration.rs` — `inventory::submit! { RegisteredAgent(register_agent! { family, phase, importance, after, fixed_step, [default_level,] lanes }) }`. No agent type: the family's agent is the one `LaneAgent`, built from the registration; the scheduler runs its installed layer every frame and wakes it only for an event. Test the family without the scheduler through `khora_agents::lane_agent::run_frame`.
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

| Attribute | Where | Effect |
|---|---|---|
| `skip` | field | Not serialized, not inspected (GPU handles, runtime caches). |
| `domain = X` | type | `SemanticDomain` self-registered into `World`. |
| `provenance = X` | type | `Authored` (default) / `ToolAuthored` → saved in scenes; `Derived` / `Runtime` → never in a scene, not copied on duplicate. |
| `resumable` | type | A `Runtime` component a **game save** keeps so play resumes (a behavior's `Guard#state`). |
| `formerly = "Old"` | type or field | Old name a save may hold; repeatable. On a field it becomes a serde alias; on a type the load reports `Renamed`. |
| `layout = "soa"` | type | Field-split SoA column (all-`f32` structs only). |
| `no_serializable` | type | **Removes the `ComponentRegistration`** — never saved, inspected, or reachable by name. Not a synonym for runtime state; prefer a provenance. |

- The derive self-registers via `inventory`. Only generics and hand-written impls stay explicit in `World::new`.
- Removing a component type: declare it retired (`inventory::submit!{ RetiredComponent { name: "Old" } }`), or every save naming it fails to load.

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
- Pre-commit: `cargo gate` (fmt + clippy + test + doc). A `secret-scan` pre-commit hook (installed by the AI installer) blocks commits containing secrets.
- CI: GitHub Actions in `.github/workflows/`.
- Commit messages: imperative mood, optional prefix (`feat:`, `fix:`, `refacto:`, `docs:`).
- Never push without explicit user permission.

## 9 — Documentation

- mdBook source in `docs/src/*.md` (`mdbook build docs/`). Custom Ayu-dark theme in `docs/theme/custom.css`.
- Document in existing files in place — **no separate ADR section/files** for this project.
- Public APIs require rustdoc; `# Examples` blocks compile via `cargo test --doc`. Use intra-doc links (`[Type]`).
- Keep mdBook chapters in sync with the code: change a public API → update the chapter in the same change.
- For any **design / UI-UX** task (editor visuals, docs visual language, game UI), use **`/impeccable`**.

## 9b — Comments

**Code says how; comments say what and why.**

1. **Doc comment (`///`) = the contract.** First paragraph: *one sentence*, at most 2 lines (about
   15 words). A function's summary starts with a verb ("Returns…", "Spawns…"); a type's is a noun
   phrase. Then, if needed, one short paragraph of what the signature cannot say: preconditions,
   side effects, ordering ("call after…"), units, invariants, locks held. Then the standard
   sections: `# Errors`, `# Panics`, `# Safety`, `# Examples`. Aim for 6 prose lines or fewer;
   more only for a public entry point with an example.
2. **Never the implementation in a doc.** No step lists, no "the scheduler then…". If the reader
   needs the mechanism, it is a book page.
3. **Every `pub` item is documented.** A private item gets a doc only when its name and
   signature leave something non-obvious, usually in one line.
4. **Inline comment (`//`) = why.** A reason, a trap, an invariant relied on. Four lines at most,
   full sentences. No numbered steps: a function with a step list is split into named functions.
5. **Never in a comment**: history ("previously", "replaces", "no longer"), plan or review labels
   (`Phase N`, `step 3f`, `P1`, `R7`), restating the code, rules copied from `RULES.md`, design
   essays. History goes in the commit message.
6. **One home per fact.** Design, rationale, frame order and protocols live in `docs/src`. A
   comment points there with `See the book: concepts/the-frame.md, "Heading".` (a path and a
   heading, both checked).
7. **`//!` module doc**: 1–3 lines saying what the module holds. A crate's `lib.rs` may say more.
8. **`// SAFETY:`** on every `unsafe` block, unchanged. The 13-line licence header stays.

Checked by `xtask/tests/layout.rs`: `no_plan_labels_in_comments` and `book_citations_resolve`
(strict), `doc_summary_is_short` and `inline_comments_are_short` (ratchets).

**Checklist before commit**
- Each summary is one sentence and fits in 2 lines.
- Errors, panics and unsafety are in their sections, not in prose.
- No doc describes how the body works; no comment restates a line.
- No plan label, no history, no step numbering.
- Anything longer than a paragraph is in the book, linked by path and heading.

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
