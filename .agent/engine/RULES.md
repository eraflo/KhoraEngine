# Khora Engine — Rules (engine profile)

Hard constraints for all code changes. Read before editing. Pairs with [`conventions.md`](./conventions.md)
and [`security-privacy.md`](./security-privacy.md).

- Document — Khora Engine Rules v2.0
- Status — Authoritative

---

## 1 — Must always

- Validate every change with `cargo gate` (fmt, clippy, tests, doc tests — what CI runs) before committing; iterate with `cargo t -p <crate> [filter]`. See `skills/build-and-test` for where the build time goes.
- Use the engine's own math types `khora_core::math::{Vec3, Mat4, Quaternion, LinearRgba, …}` — never raw `glam`.
- Naming: `snake_case` (Rust), `PascalCase` (types), `kebab-case` (crate names). See [`conventions.md`](./conventions.md).
- Add `#[cfg(test)]` unit tests for any new public function.
- Add a `// SAFETY:` comment on every `unsafe` block explaining why the invariant holds.
- Log via `log::{info,warn,error,debug,trace}` — never `println!` / `eprintln!`. **Sole exception:** the engine's own log sink (`EditorLogCapture::log` in `khora-editor/src/log_capture.rs`) writes to stderr with `eprintln!` because a `log::Log` implementation calling `log::*` would recurse infinitely.
- Validate at system boundaries (user input, file I/O, GPU errors). Trust internal API contracts.
- Write WGSL for the wgpu backend — no GLSL or SPIR-V.

## 2 — Build & test

- Compile with zero warnings at the configured lint level; clippy clean (part of `cargo gate`).
- All workspace tests must pass before declaring work complete (~2520 today; treat the live count as truth).
- No Vulkan validation errors when running `cargo run -p sandbox`; confirm the frame loop is clean for GPU work.

## 2b — Build cost (read before adding a dependency, a feature or a target)

What makes the build slow and `target/` large is how many times the whole engine is compiled and linked,
not how many lines it has. In 2026-11 it reached 57 min per gate and 50 GB, until the causes below were removed.
Measurements and levers: `docs/research/2026-11-06_build-time-and-target-size.md`.

- **No optional feature in a workspace crate.** No `[features]` table in a workspace crate's manifest, and no
  `features = [...]` on a dependency that points to a workspace crate. Cargo identifies a crate by its feature
  set: `cargo gate` and `cargo t` pass `--all-features`, `cargo run -p khora-editor` does not. A single
  optional feature low in the graph builds the whole engine twice. The `xtask` manifest checks enforce this.
- **One feature set per third-party dependency.** Declare a dependency once, in `[workspace.dependencies]`
  when several crates use it, with every feature it needs. Never enable extra features of it from
  `[dev-dependencies]` or from a single crate: tests and runs would then build it, and everything above it,
  twice. Check with `cargo tree -e features -i <dep>` before adding one, and prefer a dependency already in
  the tree to a new one.
- **The `workspace-hack` unifies what is left** (`cargo-hakari`, config `.config/hakari.toml`): `cargo t -p
  <crate>` resolves features for that crate alone, `cargo gate` for the whole workspace, and without it they
  build two different engines. After adding, removing or changing a dependency, run `cargo hakari generate`
  then `cargo hakari manage-deps`, and commit what they change; `cargo gate` fails until you do. Never edit
  `workspace-hack/Cargo.toml` by hand. The floor crates and `xtask` stay out of it (they would pull in
  `windows`, `tokio`…), as the config says.
- **Heavy dependencies stay high.** Never add wgpu, egui, rapier, winit or `windows` to a floor crate
  (`khora-core`, `khora-data`, `khora-script`, `khora-control`): each one reaches every test binary above it.
- **No new test binary per file.** A crate's integration tests are modules of its one `tests/it/` binary.
  Each test binary links the whole engine. A separate binary needs a reason written at its top, such as a
  global allocator.
- **A target with no test sets `test = false`.** For example a bin or an example program. The gate runs
  cargo's default test targets, which leave it out. `--all-targets` would build it anyway, because an
  explicit selection overrides `test = false`. An `xtask` check fails if a test is written in such a target.
- **No debuginfo in dev builds** (`debug = 0`). Don't turn it back on in `Cargo.toml`: build with
  `CARGO_PROFILE_DEV_DEBUG=2` (or `line-tables-only`) when you need a debugger.

## 3 — Architecture rules

- Respect the CLAD dependency graph; dependencies flow **downward only**. Each
  tier may depend on anything above it and nothing below:

  | Tier | Crates | Depends on |
  |---|---|---|
  | floor | `khora-core`, `khora-macros` | nothing |
  | on the floor | `khora-data`, `khora-control`, `khora-script`, `khora-telemetry`, `khora-infra`, `khora-tool-ui` | core (+ `macros` for data/script, + `data` for control) |
  | middle | `khora-io`, `khora-lanes` | core, data, script, telemetry / core, macros (the lane derives), data, script |
  | strategists | `khora-agents` | core, data, lanes, script (`khora-infra` only as a dev-dependency, for tests) |
  | façade | `khora-sdk` | agents, control, core, data, infra, io, lanes, telemetry |
  | apps | `khora-editor`, `khora-hub`, `khora-runtime` | sdk (+ `tool-ui` for editor and hub) |

  The two easy to get backwards: **`khora-agents` never depends on `khora-infra`**
  (agents reach backends through core traits in the `Runtime`), and
  **`khora-infra` depends on `khora-core` only** — it is a floor
  tenant, not a top layer. Never introduce a cycle; `cargo tree` is the arbiter.
- Abstract traits live in `khora-core`. Concrete backends live in per-backend subfolders under `khora-infra` (`graphics/wgpu/`, `physics/rapier/`, `audio/cpal/`, `ui/taffy/`, …).
- Change a `khora-core` trait only when you also update every downstream implementation.
- Keep GPU resources behind abstract IDs (`TextureId`, `BufferId`, `PipelineId`). Never expose raw wgpu handles in public APIs.
- **Lanes MUST consume Views from the `LaneBus`** (`khora-core/src/lane/bus.rs`), not query the World directly. The domain `Flow` (`khora-data/src/flow/`) is the only legitimate producer of those Views.
- **Adapt the HOW, never the WHAT.** Automatic adaptation (DCC / GORNA / `Flow` / AGDF) changes only *representation* — strategy, quality, memory layout. It MUST NEVER change *game semantics* (which components an entity has, simulation-observable behavior).
- **Structural mutations are forbidden inside Lanes.** Representation-only layout changes (AGDF, e.g. SoA↔AoSoA) belong to the data layer's self-maintenance; Flows are **read-only projectors**. Semantic structural change (attach/detach gameplay components) is developer-authored — opt-in policy, `DataSystem` invariants, or explicit editor/script actions.
- **The DCC decides how much; the agent decides how.** Per epoch the DCC lends each agent an **envelope** (time per device, memory) — never a row, a developer's pin aside; the agent chooses the row whose price fits, and reports after every run. The DCC's thread never touches an agent: allocations are lent at a frame start by the scheduler.
- **Agents stay strategists.** A family's lane runs as a **layer** installed at the family's **place** (its phase and pass); the scheduler applies it every frame without the agent, and wakes the agent only for an event (`Wake`). The agent then prepares a new layer beside the installed one, swapped in at a frame start at the same place, or reinstates one kept. No per-frame state, no owning a Flow, no buffering outputs. The descent is `Control → Agent → Lane → Data`.
- **Engine-tick wiring is data-driven** — to add an invariant register a `DataSystemRegistration`; to add a Flow use `register_flow!`; to add an asset decoder register an `AssetDecoderRegistration`. Never wire a system manually in engine code.

## 4 — Code quality

- Never `unwrap()` on a fallible GPU or I/O operation. Use `Result`, `?`, or `map_err`.
- Never use `Box<dyn Any>` downcasting as a substitute for proper trait design.
- Never store mutable global state. Use a lane's `#[deck]` outputs (the `OutputDeck`), a runtime resource or ECS components.
- `Arc<dyn Trait>` for cross-crate shared references; `Arc<Mutex<T>>` for shared mutable state.
- One primary type per file (`mat4.rs` → `Mat4`).

## 5 — Concurrency

- Never use `std::thread::spawn` directly. Concurrency goes through the DCC agent system. The DCC schedules; each family's place runs its layer (a `Lane`), the scheduler running it; agents only choose. **Exceptions:** (1) the DCC's own cold-path tick thread (`DccService::start` in `khora-control/src/dcc_service/decision_loop.rs`) — it *is* the concurrency authority this rule routes everything else through, spawned once at startup; (2) the scheduler's **persistent worker pool** (`WorkerPool` in `khora-control/src/worker_pool.rs`), spawned once in `ExecutionScheduler::new` and joined on `Drop`, which runs every member of a concurrent wave as a `'static` job (reaching its inputs through `Arc<LaneBus>`/`Arc<Runtime>`; a wave of one runs inline on the calling thread). No agent reaches the `World`, so it never crosses a thread boundary and no `unsafe`/lifetime-transmute is needed. The pool threads are scheduler-owned and never escape its lifetime. Test code may also spawn threads for isolation. A tool's startup has no exception: the engine boots first, and the tool files its heavy work (project index, scene, script compile) with the engine's families by ticket, its loading film waiting on them through the SDK's `Readiness`.
- **A backend a concurrent wave can reach never deadlocks, by construction.** Lanes prepare and record in concurrent waves, so the device, the pipeline system, an atlas — every runtime backend or resource a lane reaches — is called from several threads at once. Its locks are **leaves** (held only to look up, insert or remove, never across a call out), or sit above leaves in one declared order that only descends (the `PipelineSystem` holds its lock across device calls; the device's single `Registry` lock is the leaf below it). Nothing calls back up while holding a lock. A new backend states its order in its trait's docs (`GraphicsDevice`, `PipelineSystem` in `khora-core`), and a concurrency test on the real implementation guards it (`khora-infra` `device/concurrency_tests.rs`).
- An agent declares **one** thing to the scheduler: `Agent::contention()` — the `OutputDeck` slots it writes, and the `Runtime` resources, services and backends it **reads** or **locks**. No agent reaches the `World` (its lanes read Views, write the deck), so there is no world access to declare. Two agents share a concurrent wave only if their contentions are disjoint; reads do not conflict with reads.
- Never reach `context.runtime.{resources,services,backends}` from an agent or a lane. Go through `EngineContext::resource` (reading) or `::locked` (taking a lock) — a lane through its typed inputs — which refuse what the contention did not declare. Declaring more than you touch is not harmless: it silently serialises a wave that could have run concurrently, and no test reddens.

## 6 — Subsystem boundaries

- Physics through the `PhysicsProvider` trait + physics lane. Never call Rapier directly from agents.
- Audio through the `AudioDevice` trait + spatial mixing lane. Never call CPAL directly.
- UI layout through the `LayoutSystem` trait. Never call Taffy directly from agents.
- Reference loaded assets via `AssetHandle<T>` / `HandleComponent<T>`. Never store raw asset data inline.
- Save and load scenes as **records** through `SerializationService` (`khora-io/src/serialization.rs`): components by registered name, fields by name, entities by `PersistentId`, page-shaped. The `SerializationGoal` picks the encoding; it never changes what is saved. Never save raw page memory; never write a payload migration. Detail: [`reference/ecs-data.md`](./reference/ecs-data.md) §Persistence.
  - `FastestLoad` is the exception that proves it: a positional **snapshot** (`KH_SNAPSHOT_V1`) bound to every Rust component's schema fingerprint, refused whole on any mismatch. It is a same-build cache (the editor's Play/Stop restore) — **never a project's only copy**. A script-declared component is snapshotted with its fields and restored onto the live declaration by name (a hot reload during Play may have relaid it out).
  - Loads are **atomic**: `prepare` reads, checks and reserves every id; `Prepared::commit` adds; `Prepared::abandon` gives the ids back. A failed load leaves the world exactly as it was. What a load adapted is returned as a `LoadReport` (`ReportKind`: `Defaulted`, `Dropped`, `Renamed`, `Widened`, `Retired`, `NotSaved`, `DeadReference`, `RemovedFromScene`, `Unusable`, `NotLoaded`) — surface it, don't swallow it.
  - A load can be **prepared off the frame** without the `World`: `prepare_with(&LoadCatalog, ReservationHold, &record)`
    (and `prepare_snapshot_with`, `prepare_game_with`). `World::load_catalog()` is what a prepare reads of the world;
    its generation moves when a component is registered, declared, relaid out, moved or given another provenance.
    `World::hold_reservations()` holds the reserved ids across frames until `commit` or until the hold is dropped.
    A load committed after its world's components changed is prepared again against them; one committed into
    another world adds nothing (`NotLoaded`).
  - A tool or game that loads a scene while frames run **files** it with the `SceneLoadHub` (`khora-io/src/scene_loads.rs`)
    and reads its outcome by `LoadTicket`. The `loading` family prepares it off the frame; `scene_intake` commits it at
    the boundary. Loads land in the order they were filed, and a cancelled one lands nothing. Never load a scene on the
    frame from a running tool. The synchronous `load_world`/`replace_world`/`load_game` are for setup, tests and
    scratch worlds.
  - An unknown component name fails the **whole** load. Rename a type or a field only with `#[component(formerly = "…")]` (on a field it becomes a serde alias). Remove a type only by declaring it retired: `inventory::submit!{ RetiredComponent { name: "…" } }` (`khora-data/src/scene/retired.rs`).
  - A component **declared while the engine runs** (an Ergon `component`, `ComponentKey::Declared`) has no registration, so a record holding one carries its declaration (`SceneRecord.declared`, `khora-data/src/scene/declared.rs`): name, domain, fields. Such a name is *known by the file*; a load reads its fields onto the world's declaration by name, or registers the file's at `commit` (never at `prepare`). A file declaration named like a Rust component fails the load.
  - A scene holds what was authored; a game save (`SaveRecord`) holds only how the running world differs from its base scene, merged three-way on load. Never put engine-written state in a scene.
  - Prefab members are keyed by `PersistentId::within(root, inner)`. Its namespace (`WITHIN`, `khora-core/src/ecs/persistent_id.rs`) is fixed forever — changing it re-keys every instance in every saved scene.
  - Older scene formats are refused (`SceneFileReadError::OldFormat`); there is no v1 reader to extend.
- Never inline WGSL source as a Rust `const`/`static`. Shaders are `.wgsl` files under `crates/khora-infra/src/graphics/shader/shaders/` (`pipelines/` entry points, `lib/` reusable modules), embedded with `include_str!` and composed by the `PipelineSystem` backend (`khora-infra/src/graphics/wgpu/pipeline_system/mod.rs`) using `naga_oil` `#import`. Lanes resolve a pipeline **by name** (`khora::pipelines::grid`), never by handing over source.
  - **One exception, and it is the only one:** `EGUI_WGSL` in `khora-infra/src/graphics/shader/mod.rs` (its `.wgsl` file sits in the shaders tree with the others, under `standalone/`). Its consumer, the egui overlay, takes a raw string rather than a pipeline handle. No new `_WGSL` constant should appear — UI text is drawn by the UI lane through `khora::pipelines::ui_text` like any other pipeline.
  - There is **no `ShaderRegistry` type** — earlier revisions of this file named one. The composition point is the `PipelineSystem` backend.

## 7 — Components & ECS

- `#[derive(Component)]` for all ECS components; the macro generates the `SerializableX` mirror + `From` conversions and self-registers a `ComponentRegistration` via `inventory`.
- `#[component(skip)]` for non-serialized fields (GPU handles, runtime caches).
- **A component that must be watched is declared by its observer, never polled.** A `DataSystem` or `Flow` that reacts to changes lists the components in its registration's `observes` and reads `Changed<T>` since its last run (`SystemContext::since`); it never diffs values frame to frame. Writes go through the `Mut<T>` guard or the by-value paths, which stamp the rows they wrote — a write route that bypasses them is a missed change. A value recomputed every frame is written with `set_if_neq`. Detail: [`reference/ecs-data.md`](./reference/ecs-data.md) §Change detection.
- `#[component(no_serializable)]` **removes the `ComponentRegistration`** (`khora-macros/src/component.rs:255-261`): the component is never saved, never inspected, never reachable by name. It is not "runtime state" — use a provenance for that. No shipped component uses it today.
- Choose a **provenance** (`ComponentProvenance`, `khora-data/src/ecs/component_registry.rs`): `Authored` (default) and `ToolAuthored` are saved in scenes; `Derived` and `Runtime` never are. Add `#[component(resumable)]` to `Runtime` state a **game save** must keep so play resumes (no Rust component does today; a behavior's state companion `Guard#state` is registered resumable at run time).
- Declare a component's domain with `#[component(domain = Physics)]`. Only generics (`HandleComponent<T>`) and hand-written impls stay explicit in `World::new`.
- A component that can hold values the engine cannot use (a camera's near plane at 0, a light's range of 0) declares `#[component(check = Type::check)]`, a `fn(&Self) -> Vec<FieldProblem>` naming each field by its dotted JSON path. A check **never rewrites nor refuses** a value: a load reports `ReportKind::Unusable` (the value is kept), the running world reports to `khora_core::problems::Problems` as `ProblemSource::Data` (the `component_checks` DataSystem), the inspector shows the message under the field. Whatever is built from such data uses the math's `try_*` constructors (`Mat4::try_perspective_rh_zo`, …), never the asserting ones: bad data renders nothing and says why, it never panics the frame.

## 7b — Ergon (scripting)

- Scripts run only while the scripting family (`khora_core::script::SCRIPT`) is on. The engine has no mode: the editor switches the family off while editing, through the `Activation` resource. The script DataSystems and `ScriptFlow` are `Role::Serves(SCRIPT)`, so they are skipped while it is off, as the family's agent is. An editor editing a scene runs no script.
- A new statement kind must give its key in `khora-script/src/bytecode/keys.rs` (every match there is exhaustive) and name its sites (`bytecode/sites.rs`) — otherwise a suspended frame cannot be found again after an edit.
- An Ergon native that reaches beyond the world — a file, the network, a process — declares a capability in `#[ergon_fn]` and runs only in a trusted project (`ProjectTrust::Trusted`). No native does today, so there is no capability manifest yet: it comes with the first such native.
- A new `Instruction` needs an arm in `bytecode/fingerprint.rs` (exhaustive match) that hashes **what it names** (callee, native, literal, field — never an index), and a deliberate fuel cost (`Instruction::cost` in `vm/instruction.rs`; a native is charged its declared cost).

## 8 — Must never

- Write a new agent type. A new subsystem is a **family**: its lanes and one `register_agent!` registration (phase, importance, lanes, default level); its agent is the one `LaneAgent`, built by `lane_agent`. An agent implements **only** `Agent` — no `start/stop`, builders, or accessors; private free functions in the module file are fine. (No hand-written agent is left: rendering and scripting are families too.)
- Give an agent more than one family, or any job other than choosing its row, answering GORNA, and installing its layers. An agent has no `execute`: the scheduler runs its family's place, and wakes the agent only for an event. `LaneAgent` is the only implementor outside tests (the xtask check `only_lane_agent_implements_agent`).
- Offer one `StrategyId` twice in a family — boot refuses it, and a family's rows, pins and layers are named by it.
- Use an `Agent` for a subsystem that doesn't need GORNA negotiation — use a direct service (`AssetService`, `SerializationService`, `EcsMaintenance`).
- Bypass the `Lane` abstraction for hot-path work.
- Add concrete backend logic to `khora-core`.
- Commit code with Vulkan validation errors or wgpu warnings.
- Commit or push secrets / confidential info — see [`security-privacy.md`](./security-privacy.md).

## 9 — Boundaries (do-not-touch)

- Only modify files inside the `KhoraEngine` workspace.
- **Never edit generated AI wrappers** — `CLAUDE.md`, `AGENTS.md`, `GEMINI.md`, `.github/copilot-instructions.md`, `.claude/`, `.cursor/`, `.gemini/`. They are regenerated from `.agent/engine/` by the installer. Edit the canonical source here, not the copies.
- Don't touch `target/`, `Cargo.lock` (unless intentionally bumping deps), `.codegraph/`, `.gitagent/`, or vendored code.
- Don't modify `.github/workflows/` without asking.

## 10 — Permission model

- **Stop at every step (invariable, owner 2026-11-07).** Each phase or sub-step of a plan ends with a
  report and a wait for the owner to check what was done; never chain into the next step on your own.
  See [`workflow-rpi.md`](./workflow-rpi.md).

- **No prompt:** read any file, run `cargo gate` / `cargo t` / `cargo run`, query the codegraph MCP, lint.
- **Ask first:** installing dependencies, any `git` write op (commit/push/branch/reset), deleting files or branches, full release builds, modifying CI.
- **Never without explicit user permission:** push to git, create PRs, modify CI/workflows.

---

## Decisions (rationale)

- **One agent per family** — keeps each agent focused on one negotiation surface.
- **Lanes are layers** — an agent off the hot path costs nothing on a quiet frame, a swap is prepared beside and lands at a frame start (no first-frame hitch), and its measured prepare time is what a switch of row costs.
- **GPU IDs over raw handles** — decouples public APIs from wgpu version drift.
- **Shaders as files, never strings** — editable, reviewable, hot-reloadable.
- **Backend code in `khora-infra` only** — `khora-core` stays portable and trait-only.
- **No `std::thread::spawn`** — otherwise telemetry and budgets become meaningless.
- **No `unwrap()` on GPU paths** — one panic in render code crashes the frame loop.

*End of rules.*
