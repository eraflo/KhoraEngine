# Khora Engine — Rules

Hard constraints for all code changes. Read before editing.

- Document — Khora Engine Rules v1.0
- Status — Authoritative
- Date — May 2026

---

## Contents

1. Must always
2. Build & test
3. Architecture rules
4. Code quality
5. Concurrency & threading
6. Subsystem boundaries
7. Component & ECS rules
8. Must never
9. Output constraints
10. Interaction boundaries

---

## 01 — Must always

- Run `cargo build` and `cargo test --workspace` after any code change.
- Use the engine's own math types `khora_core::math::{Vec3, Mat4, Quat, LinearRgba}` — never raw `glam`.
- Follow naming conventions: `snake_case` for Rust, `PascalCase` for types, kebab-case for crate names. See [`conventions.md`](./conventions.md).
- Add `#[cfg(test)]` unit tests for any new public function.
- Add a `// SAFETY:` comment on every `unsafe` block, explaining why the invariant holds.
- Use `log::info`, `log::warn`, `log::error` for logging — never `println!` or `eprintln!`.
- Validate at system boundaries (user input, file I/O, GPU errors). Trust internal API contracts.
- Write WGSL for the wgpu backend — no GLSL or SPIR-V.

## 02 — Build & test

- The workspace must compile with zero warnings on the configured lint level.
- All `~470` workspace tests must pass before declaring work complete.
- No Vulkan validation errors when running `cargo run -p sandbox`.
- For GPU work, run the sandbox once and confirm the frame loop is clean.

## 03 — Architecture rules

- Respect the CLAD dependency graph: `khora-core` → `khora-data` / `khora-control` → `khora-lanes` → `khora-agents` → `khora-infra` → `khora-sdk`. Dependencies flow downward only.
- Never introduce circular dependencies between crates.
- Abstract traits live in `khora-core`. Concrete backends live in per-backend subfolders inside `khora-infra` (`graphics/wgpu/`, `physics/rapier/`, `audio/cpal/`, `ui/taffy/`, etc.).
- Modify `khora-core` trait interfaces only when you also update every downstream implementation.
- Keep GPU resources behind abstract IDs (`TextureId`, `BufferId`, `PipelineId`). Never expose raw wgpu handles in public APIs.

## 04 — Code quality

- Never call `unwrap()` on a fallible GPU or I/O operation. Use `Result`, `?`, or `map_err`.
- Never use `Box<dyn Any>` downcasting as a substitute for proper trait design.
- Never store mutable global state. Use `LaneContext` slots or ECS components.
- Use `Arc<dyn Trait>` for cross-crate shared references.
- Use `Arc<Mutex<T>>` for shared mutable state (e.g., `WgpuGraphicsContext`).
- One primary type per file (`device.rs` → `WgpuDevice`).

## 05 — Concurrency & threading

- Never use `std::thread::spawn` directly. Concurrency goes through the DCC agent system.
- Per-frame work runs through agents and the `Lane` trait. The DCC schedules; agents execute.

## 06 — Subsystem boundaries

- Route physics through the `PhysicsProvider` trait and `StandardPhysicsLane`. Never call Rapier directly from agents.
- Route audio through the `AudioDevice` trait and `SpatialMixingLane`. Never call CPAL directly.
- Route UI layout through the `LayoutSystem` trait. Never call Taffy directly from agents.
- Use `AssetHandle<T>` for referencing loaded assets. Never store raw asset data inline.
- Implement serialization through the three-strategy pattern (Definition / Recipe / Archetype) via `SerializationGoal`.
- Never inline WGSL shader source as a Rust `const` or `static` string. All shaders live as `.wgsl` files under `crates/khora-lanes/src/render_lane/shaders/`.

## 07 — Component & ECS rules

- Use `#[derive(Component)]` for all ECS components. The macro auto-generates the `SerializableX` mirror struct and `From` conversions.
- Use `#[component(skip)]` on fields that must not be serialized (GPU handles, runtime state).
- Use `#[component(no_serializable)]` for components that need a manual `SerializableX` (unit structs, trait objects).
- Register components via `inventory::submit!` in `khora-data/src/ecs/components/registrations.rs`. Use the `register_components!` macro for DRY registration.

## 08 — Must never

- Add a method outside of the `Agent` trait to an agent struct. Agents implement **only** `Agent` and `Default`. No `start()`, `stop()`, builder methods, or accessor helpers. Construction goes through `Default::default()`. Private free functions in the same module file are acceptable for internal helpers.
- Give an agent responsibility for more than one `LaneKind`. Lanes do one thing. One agent per `LaneKind`.
- Give an agent any responsibility other than lane selection, GORNA budget negotiation, and `Lane::execute()` dispatch.
- Use an `Agent` for subsystems that don't need GORNA strategy negotiation. Use a direct service instead (e.g., `AssetService`, `SerializationService`, `EcsMaintenance`).
- Bypass the `Lane` abstraction for hot-path work. All render, physics, audio, asset pipelines go through `Lane::execute()`.
- Add concrete (backend-specific) logic to `khora-core`. Define abstract traits and types in `khora-core`. Implement them in per-backend subfolders inside `khora-infra`.
- Commit code with Vulkan validation errors or wgpu warnings.
- Push to git, create PRs, modify CI, or modify `.github/workflows/` without explicit user permission.
- Delete files or branches without explicit confirmation.

## 09 — Output constraints

- Code changes include the file path and exact line context.
- Use markdown code blocks with language identifiers (` ```rust `, ` ```wgsl `, ` ```toml `).
- Keep explanations under five sentences unless the user asks for detail.
- When summarizing work, list files changed and tests affected.

## 10 — Interaction boundaries

- Only modify files within the `KhoraEngine` workspace.
- Do not push to git, create PRs, or modify CI without explicit permission.
- Do not delete files or branches without confirmation.
- Do not modify `.github/workflows/` without asking.

---

## Decisions

### We said yes to
- **One agent per `LaneKind`.** Forcing the split keeps each agent focused on one negotiation surface.
- **GPU IDs over raw handles.** Decouples public APIs from wgpu version drift.
- **Shaders as files, never strings.** Editable, reviewable, hot-reloadable.
- **Backend code in `khora-infra` only.** `khora-core` stays portable and trait-only.

### We said no to
- **Free-form agent shapes.** No builder methods, no accessor helpers, no `start/stop` — `Default::default()` plus the `Agent` trait, nothing more.
- **Direct `std::thread::spawn`.** All concurrency flows through DCC. Otherwise, telemetry and budgets become meaningless.
- **`unwrap()` on GPU paths.** A single panic in render code crashes the frame loop; we always handle the error.

---

*End of rules.*
