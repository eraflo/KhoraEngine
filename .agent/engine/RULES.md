# Khora Engine — Rules (engine profile)

Hard constraints for all code changes. Read before editing. Pairs with [`conventions.md`](./conventions.md)
and [`security-privacy.md`](./security-privacy.md).

- Document — Khora Engine Rules v2.0
- Status — Authoritative

---

## 1 — Must always

- Run `cargo build` and `cargo test --workspace` after any code change. **Primary check:** `cargo test --workspace`.
- Use the engine's own math types `khora_core::math::{Vec3, Mat4, Quaternion, LinearRgba, …}` — never raw `glam`.
- Naming: `snake_case` (Rust), `PascalCase` (types), `kebab-case` (crate names). See [`conventions.md`](./conventions.md).
- Add `#[cfg(test)]` unit tests for any new public function.
- Add a `// SAFETY:` comment on every `unsafe` block explaining why the invariant holds.
- Log via `log::{info,warn,error,debug,trace}` — never `println!` / `eprintln!`. **Sole exception:** the engine's own log sink (`EditorLogCapture::log` in `khora-core/src/ui/editor/log_capture.rs`) writes to stderr with `eprintln!` because a `log::Log` implementation calling `log::*` would recurse infinitely.
- Validate at system boundaries (user input, file I/O, GPU errors). Trust internal API contracts.
- Write WGSL for the wgpu backend — no GLSL or SPIR-V.

## 2 — Build & test

- Compile with zero warnings at the configured lint level; `cargo clippy --workspace` clean.
- All workspace tests must pass before declaring work complete (~586 today; treat the live count as truth).
- No Vulkan validation errors when running `cargo run -p sandbox`; confirm the frame loop is clean for GPU work.

## 3 — Architecture rules

- Respect the CLAD dependency graph; dependencies flow **downward only**: `khora-core` → `khora-data` / `khora-control` → `khora-lanes` → `khora-agents` → `khora-infra` → `khora-sdk`. Never introduce a cycle.
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
- One primary type per file (`device.rs` → `WgpuDevice`).

## 5 — Concurrency

- Never use `std::thread::spawn` directly. Concurrency goes through the DCC agent system. The DCC schedules; agents execute. Per-frame work runs through agents and the `Lane` trait. **Sole exception:** the DCC's own cold-path tick thread (`DccService::start` in `khora-control/src/service.rs`) — it *is* the concurrency authority this rule routes everything else through, spawned once at startup. Test code may also spawn threads for isolation.

## 6 — Subsystem boundaries

- Physics through the `PhysicsProvider` trait + physics lane. Never call Rapier directly from agents.
- Audio through the `AudioDevice` trait + spatial mixing lane. Never call CPAL directly.
- UI layout through the `LayoutSystem` trait. Never call Taffy directly from agents.
- Reference loaded assets via `AssetHandle<T>` / `HandleComponent<T>`. Never store raw asset data inline.
- Serialize through the three-strategy pattern (Definition / Recipe / Archetype) via `SerializationGoal`.
- Never inline WGSL source as a Rust `const`/`static`. Shaders are `.wgsl` files under `crates/khora-lanes/src/render_lane/shaders/` (`pipelines/` entry points, `lib/` reusable modules), composed via `ShaderRegistry` + `naga_oil` `#import`.

## 7 — Components & ECS

- `#[derive(Component)]` for all ECS components; the macro generates the `SerializableX` mirror + `From` conversions and self-registers via `inventory`.
- `#[component(skip)]` for non-serialized fields (GPU handles, runtime state); `#[component(no_serializable)]` for manual mirrors.
- Declare a component's domain with `#[component(domain = Physics)]`. Only generics (`HandleComponent<T>`) and hand-written impls stay explicit in `World::new`.

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

- **No prompt:** read any file, run `cargo build` / `cargo clippy` / single-test runs, query the codegraph MCP, lint.
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
