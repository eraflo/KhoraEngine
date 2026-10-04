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

## 3 — Architecture rules

- Respect the CLAD dependency graph; dependencies flow **downward only**. Each
  tier may depend on anything above it and nothing below:

  | Tier | Crates | Depends on |
  |---|---|---|
  | floor | `khora-core`, `khora-macros` | nothing |
  | on the floor | `khora-data`, `khora-control`, `khora-script`, `khora-telemetry`, `khora-infra`, `khora-tool-ui` | core (+ `macros` for data/script, + `data` for control) |
  | middle | `khora-io`, `khora-lanes` | core, data, script, telemetry / core, data, io, script |
  | strategists | `khora-agents` | everything above, **including `khora-infra`** |
  | façade | `khora-sdk` | agents, control, core, data, infra, io, lanes, telemetry |
  | apps | `khora-editor`, `khora-hub`, `khora-runtime` | sdk (+ `tool-ui` for editor and hub) |

  The two easy to get backwards: **`khora-agents` depends on `khora-infra`**, not
  the reverse, and **`khora-infra` depends on `khora-core` only** — it is a floor
  tenant, not a top layer. Never introduce a cycle; `cargo tree` is the arbiter.
- Abstract traits live in `khora-core`. Concrete backends live in per-backend subfolders under `khora-infra` (`graphics/wgpu/`, `physics/rapier/`, `audio/cpal/`, `ui/taffy/`, …).
- Change a `khora-core` trait only when you also update every downstream implementation.
- Keep GPU resources behind abstract IDs (`TextureId`, `BufferId`, `PipelineId`). Never expose raw wgpu handles in public APIs.
- **Lanes MUST consume Views from the `LaneBus`** (`khora-core/src/lane/bus.rs`), not query the World directly. The domain `Flow` (`khora-data/src/flow/`) is the only legitimate producer of those Views.
- **Adapt the HOW, never the WHAT.** Automatic adaptation (DCC / GORNA / `Flow` / AGDF) changes only *representation* — strategy, quality, memory layout. It MUST NEVER change *game semantics* (which components an entity has, simulation-observable behavior).
- **Structural mutations are forbidden inside Lanes.** Representation-only layout changes (AGDF, e.g. SoA↔AoSoA) belong to the data layer's self-maintenance; Flows are **read-only projectors**. Semantic structural change (attach/detach gameplay components) is developer-authored — opt-in policy, `DataSystem` invariants, or explicit editor/script actions.
- **Agents stay strategists** — choose a Lane given a budget, report status. No per-frame state, no owning a Flow, no buffering outputs. The descent is `Control → Agent → Lane → Data`; the agent invokes its own lane.
- **Engine-tick wiring is data-driven** — to add an invariant register a `DataSystemRegistration`; to add a Flow use `register_flow!`; to add an asset decoder register an `AssetDecoderRegistration`. Never wire a system manually in engine code.

## 4 — Code quality

- Never `unwrap()` on a fallible GPU or I/O operation. Use `Result`, `?`, or `map_err`.
- Never use `Box<dyn Any>` downcasting as a substitute for proper trait design.
- Never store mutable global state. Use `LaneContext`/`OutputDeck` slots or ECS components.
- `Arc<dyn Trait>` for cross-crate shared references; `Arc<Mutex<T>>` for shared mutable state.
- One primary type per file (`mat4.rs` → `Mat4`).

## 5 — Concurrency

- Never use `std::thread::spawn` directly. Concurrency goes through the DCC agent system. The DCC schedules; agents execute. Per-frame work runs through agents and the `Lane` trait. **Exceptions:** (1) the DCC's own cold-path tick thread (`DccService::start` in `khora-control/src/dcc_service/decision_loop.rs`) — it *is* the concurrency authority this rule routes everything else through, spawned once at startup; (2) the scheduler's **persistent worker pool** (`WorkerPool` in `khora-control/src/worker_pool.rs`), spawned once in `ExecutionScheduler::new` and joined on `Drop`, which runs a concurrent wave's `AgentAccess::Isolated` agents as `'static` jobs (world-free, reaching inputs through `Arc<LaneBus>`/`Arc<Runtime>`) — a wave's `SharedWorld` agents run inline on the calling thread, so the `World` never crosses a thread boundary and no `unsafe`/lifetime-transmute is needed. The pool threads are scheduler-owned and never escape its lifetime. Test code may also spawn threads for isolation.
- An agent declares **two** things, and they answer different questions. `Agent::access()` is about the `World` alone: `Exclusive` (`&mut World`, always a wave of its own), `SharedWorld` (`&World`), `Isolated` (world-free, so eligible for the worker pool). `Agent::contention()` is about everything else — the `OutputDeck` slots it writes, and the `Runtime` resources, services and backends it **reads** or **locks**. Two agents share a concurrent wave only if their contentions are disjoint; reads do not conflict with reads.
- Never reach `context.runtime.{resources,services,backends}` from an agent. Go through `EngineContext::resource` (reading) or `::locked` (taking a lock), which refuse what `contention()` did not declare. Declaring more than you touch is not harmless: it silently serialises a wave that could have run concurrently, and no test reddens.

## 6 — Subsystem boundaries

- Physics through the `PhysicsProvider` trait + physics lane. Never call Rapier directly from agents.
- Audio through the `AudioDevice` trait + spatial mixing lane. Never call CPAL directly.
- UI layout through the `LayoutSystem` trait. Never call Taffy directly from agents.
- Reference loaded assets via `AssetHandle<T>` / `HandleComponent<T>`. Never store raw asset data inline.
- Save and load scenes as **records** through `SerializationService` (`khora-io/src/serialization.rs`): components by registered name, fields by name, entities by `PersistentId`, page-shaped. The `SerializationGoal` picks the encoding; it never changes what is saved. Never save raw page memory; never write a payload migration. Detail: [`reference/ecs-data.md`](./reference/ecs-data.md) §Persistence.
  - `FastestLoad` is the exception that proves it: a positional **snapshot** (`KH_SNAPSHOT_V1`) bound to every component's schema fingerprint, refused whole on any mismatch. It is a same-build cache (the editor's Play/Stop restore) — **never a project's only copy**.
  - Loads are **atomic**: `prepare` reads, checks and reserves every id; `Prepared::commit` adds; `Prepared::abandon` gives the ids back. A failed load leaves the world exactly as it was. What a load adapted is returned as a `LoadReport` (`ReportKind`: `Defaulted`, `Dropped`, `Renamed`, `Widened`, `Retired`, `NotSaved`, `DeadReference`, `RemovedFromScene`) — surface it, don't swallow it.
  - An unknown component name fails the **whole** load. Rename a type or a field only with `#[component(formerly = "…")]` (on a field it becomes a serde alias). Remove a type only by declaring it retired: `inventory::submit!{ RetiredComponent { name: "…" } }` (`khora-data/src/scene/retired.rs`).
  - A scene holds what was authored; a game save (`SaveRecord`) holds only how the running world differs from its base scene, merged three-way on load. Never put engine-written state in a scene.
  - Prefab members are keyed by `PersistentId::within(root, inner)`. Its namespace (`WITHIN`, `khora-core/src/ecs/persistent_id.rs`) is fixed forever — changing it re-keys every instance in every saved scene.
  - Older scene formats are refused (`SceneFileReadError::OldFormat`); there is no v1 reader to extend.
- Never inline WGSL source as a Rust `const`/`static`. Shaders are `.wgsl` files under `crates/khora-infra/src/graphics/shader/shaders/` (`pipelines/` entry points, `lib/` reusable modules), embedded with `include_str!` and composed by the `PipelineSystem` backend (`khora-infra/src/graphics/wgpu/pipeline_system/mod.rs`) using `naga_oil` `#import`. Lanes resolve a pipeline **by name** (`khora::pipelines::grid`), never by handing over source.
  - **Two exceptions, and they are the only ones:** `TEXT_WGSL` and `EGUI_WGSL` in `khora-infra/src/graphics/shader/mod.rs` (their `.wgsl` files sit in the shaders tree with the others). Their consumers (`StandardTextRenderer`, the egui overlay) take a raw string rather than a pipeline handle, and the application passes it in. No new `_WGSL` constant should appear.
  - There is **no `ShaderRegistry` type** — earlier revisions of this file named one. The composition point is the `PipelineSystem` backend.

## 7 — Components & ECS

- `#[derive(Component)]` for all ECS components; the macro generates the `SerializableX` mirror + `From` conversions and self-registers a `ComponentRegistration` via `inventory`.
- `#[component(skip)]` for non-serialized fields (GPU handles, runtime caches).
- `#[component(no_serializable)]` **removes the `ComponentRegistration`** (`khora-macros/src/component.rs:255-261`): the component is never saved, never inspected, never reachable by name. It is not "runtime state" — use a provenance for that. No shipped component uses it today.
- Choose a **provenance** (`ComponentProvenance`, `khora-data/src/ecs/component_registry.rs`): `Authored` (default) and `ToolAuthored` are saved in scenes; `Derived` and `Runtime` never are. Add `#[component(resumable)]` to `Runtime` state a **game save** must keep so play resumes (e.g. `ScriptState`).
- Declare a component's domain with `#[component(domain = Physics)]`. Only generics (`HandleComponent<T>`) and hand-written impls stay explicit in `World::new`.

## 7b — Ergon (scripting)

- Scripts run only in `EngineMode::Playing`: `ScriptAgent` is registered with `DccService::register_agent_for_mode(…, vec![EngineMode::Playing])` and `ScriptFlow` projects nothing unless `EngineMode::runs_gameplay()`. An editor editing a scene runs no script.
- A new statement kind must give its key in `khora-script/src/bytecode/keys.rs` (every match there is exhaustive) and name its sites (`bytecode/sites.rs`) — otherwise a suspended frame cannot be found again after an edit.
- A new `Instruction` needs an arm in `bytecode/fingerprint.rs` (exhaustive match) that hashes **what it names** (callee, native, literal, field — never an index), and a deliberate fuel cost (`Instruction::cost` in `vm/instruction.rs`; a native is charged its declared cost).

## 8 — Must never

- Add a method outside the `Agent` trait to an agent struct. Agents implement **only** `Agent` and `Default` — no `start/stop`, builders, or accessors. Construction via `Default::default()`. Private free functions in the module file are fine.
- Give an agent more than one `LaneKind`, or any job other than lane selection, GORNA budget negotiation, and `Lane::execute()` dispatch.
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

- **No prompt:** read any file, run `cargo gate` / `cargo t` / `cargo run`, query the codegraph MCP, lint.
- **Ask first:** installing dependencies, any `git` write op (commit/push/branch/reset), deleting files or branches, full release builds, modifying CI.
- **Never without explicit user permission:** push to git, create PRs, modify CI/workflows.

---

## Decisions (rationale)

- **One agent per `LaneKind`** — keeps each agent focused on one negotiation surface.
- **GPU IDs over raw handles** — decouples public APIs from wgpu version drift.
- **Shaders as files, never strings** — editable, reviewable, hot-reloadable.
- **Backend code in `khora-infra` only** — `khora-core` stays portable and trait-only.
- **No `std::thread::spawn`** — otherwise telemetry and budgets become meaningless.
- **No `unwrap()` on GPU paths** — one panic in render code crashes the frame loop.

*End of rules.*
