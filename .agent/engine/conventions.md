# Khora Engine — Conventions (engine profile)

Code style and project conventions. Pairs with [`RULES.md`](./RULES.md).

- Document — Khora Engine Conventions v2.0
- Status — Active

---

## Before you write

Read this before writing any code, test or doc, and decide where things go first. Detail in §3, §9b.

1. Name the file and folder of every new type and module before writing: one primary type per file, beside its siblings, never in a generic util file.
2. Look for the shared record, widget or helper before making one (`codegraph_explore`); reuse it.
3. Keep dependencies pointing down: a shared widget never imports a panel, a generic module never a domain one.
4. Rename what you change the meaning of (fn, field, file); check a new name is not taken elsewhere (two `lending.rs`, two `Mailbox`).
5. Order each file: licence header, `//!` doc, imports, constants, primary type, its impls, private helpers, tests.
6. Never put an item between a type and its `impl`s; public functions before private ones; test helpers before the tests.
7. Put each `use` in its group: `std`, then external and `khora_*` crates, then `crate::` / `super::`.
8. Write a doc as the contract: a one-sentence summary (a function's starts with a verb, about 15 words), never the mechanism.
9. Never tell history in a comment or doc ("now", "used to", "always was", "the copies disagreed"): state what holds.
10. List every error and panic in `# Errors` / `# Panics`, and every side effect (a file written, play ended) in the doc.
11. Reread the docs of what you changed: no parameter that is gone, no "entity" where an asset is served too.
12. Keep one home per fact: name the `LABEL` const, never repeat its string; a design goes to the book.
13. Give every new public fn a unit test (`#[cfg(test)]`, RULES §1), not only an integration test.
14. Put a test in the test file of the module it pins, named by the behaviour it pins.
15. Add every new public `khora_core` item to the public-paths guard (`crates/khora-core/tests/public_paths/`).
16. Update the book (`docs/src/`) in the same change when a name or behaviour it describes changes.

Before reporting, run both and leave them clean:
- `cargo xtask lint-file <every .rs file touched>`;
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items`.

## 1 — Naming

| Element | Convention | Example |
|---|---|---|
| Crates | `khora-{name}` (kebab-case) | `khora-core`, `khora-lanes` |
| Modules | `snake_case` | `spatial_mixing`, `frame_graph` |
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
- Unit tests in one inline `#[cfg(test)] mod tests`, last in the file, or in a sibling `tests.rs`; a folder `mod.rs` holds no inline tests.
- Integration tests are modules of the crate's one `tests/it/` binary (RULES §2b); a family's go in its folder there (`khora-lanes/tests/it/<family>/`, `khora-agents/tests/it/families/<family>/`).
- Backend implementations grouped per backend folder under `khora-infra/src/`: `graphics/wgpu/`, `physics/rapier/`, `audio/cpal/`, `ui/taffy/`.
- Shaders under `crates/khora-infra/src/graphics/shader/shaders/` — `pipelines/` (entry points) and `lib/` (reusable modules). One pass per pipeline file; no `.wgsl` at the shaders root.
- **Invariant `DataSystem`s**: one file per system in `crates/khora-data/src/ecs/systems/`. Discovered via `inventory::submit!{ DataSystemRegistration { … } }`. Zero wiring elsewhere. The function is `fn(&mut World, &Runtime, &mut OutputDeck, SystemContext)`; `ctx.since` is the tick of its previous run, read with `Changed<T>`, and the components it watches go in `observes`. `role` (no default) is `Role::Upkeep` (a data invariant, every frame) or `Role::Serves(family)` (skipped while that family is off, `khora_data::ecs::Role`); `ctx.resumed` tells its first run on a world, and its first run after a skip. A `Flow` declares the same as `const ROLE`, and its `select` gets `FlowContext { resumed }`. The boot refuses a `Serves` naming no registered family (`check_roles`).
- **Domain `Flow`s**: one file per flow in `crates/khora-data/src/flow/`. Register with `register_flow!(MyFlow)`. The Scheduler's Substrate Pass runs them automatically.
- **Asset decoders**: one file per decoder in `crates/khora-io/src/asset/decoders/`, registered with `inventory::submit!{ DecoderRegistration { … } }`.
- **Residencies**: one file per residency in `crates/khora-data/src/residency/`, registered with `register_residency!`.

### 3.1 — Adding a new domain

The steps, with the files and checks: the [`add-a-family`](./skills/add-a-family/SKILL.md) skill. The bricks: RULES §3.

1. **`Flow`** in `khora-data/src/flow/<domain>.rs` — implement `Flow` (read-only `select → project`), define a typed `View`, register with `register_flow!`. A Flow never mutates the World; per-frame maintenance goes in a `DataSystem`.
2. **`Lane`s and their `LaneFamily`** in `khora-lanes/src/<family>/` (`family.rs`, its lanes, an input type shared by several lanes in a file of its own) — each lane implements `khora_core::lane::Lane` (`levels`, `prepare`, `cost`, `record`, `shutdown`); it reads typed inputs (`#[derive(LaneInputs)]`: `#[bus]` the `View`, `#[deck]`, `#[resource]`, `#[locked]`, …) and writes the family's typed outputs (`#[derive(LaneOutputs)]`: `#[pass]`, `#[deck]`, `#[staged]`). Never call `world.query*`.
3. **One registration** in `khora-lanes/src/<family>/registration.rs` — `inventory::submit! { RegisteredAgent(register_agent! { family, phase, importance, after, fixed_step, [default_level,] lanes }) }`. No agent type: the family's agent is the one `LaneAgent`, built from the registration; the scheduler runs its installed layer every frame and wakes it only for an event. Test the family without the scheduler through `khora_agents::lane_agent::run_frame`.
4. A `#[pass]` output goes to a pass slot ordered by `register_pass!`, submitted by the SDK; a `#[deck]` output is read by a later phase or drained by a `Maintenance` DataSystem; an asset's copy on a device is a residency, never lane state.

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
| `check = path` | type | A `fn(&Self) -> Vec<FieldProblem>` naming the values the engine cannot use (RULES §7). |
| `layout = "soa"` | type | Field-split SoA column (all-`f32` structs only); `"aos"` is the default. |
| `no_serializable` | type | **Removes the `ComponentRegistration`** — never saved, inspected, or reachable by name. Not a synonym for runtime state; prefer a provenance. |

- A key the table does not list at that place, a key given twice, a value of the wrong kind, or `check`/`provenance`/`resumable`/`formerly` beside `no_serializable` (which registers nothing for them to act through) is a compile error on the key.
- The derive self-registers via `inventory`. Only generics and hand-written impls stay explicit in `World::new`.
- Removing a component type: declare it retired (`inventory::submit!{ RetiredComponent { name: "Old" } }`), or every save naming it fails to load.

## 5 — Logging

- `log::info!` — lifecycle events (startup, agent registration, swapchain resize).
- `log::warn!` — recoverable anomalies (asset retry, budget shortfall).
- `log::error!` — unrecoverable errors before bubbling a `Result`.
- `log::debug!` / `log::trace!` — hot-path diagnostics, gated by level.
- **Never** `println!` / `eprintln!` — they bypass the log pipeline and editor consoles.

## 6 — Errors

- Each subsystem owns its error enum: `LaneError`, `RenderError`, `SerializationServiceError`, …
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

- Book pages in `docs/src/*.md`, navigation in `docs/src/SUMMARY.md` (every page listed once). Two builds read them: the docs site (`docs/site/`, Astro Starlight: `npm ci && npm run build`) and mdBook (`mdbook build docs/`, theme `docs/theme/custom.css`) until the site reaches parity. Detail: `reference/documentation.md`.
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

Checked by `cargo xtask lint-file` (run by a hook after every write) and `xtask/tests/layout.rs`:
`no_plan_labels_in_comments`, `book_citations_resolve`, `doc_summary_is_short`,
`inline_comments_are_short`, `comments_tell_no_history`, `fn_summaries_start_with_a_verb`,
`a_type_and_its_impls_are_contiguous`, `every_public_fn_is_named_by_a_test`; and by the workspace
lints (`missing_docs`, clippy's doc lints, `undocumented_unsafe_blocks`) and the `cargo doc` step of
`cargo gate`.

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
