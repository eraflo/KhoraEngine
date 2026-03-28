# Rules

## Must Always

- Run `cargo build` and `cargo test --workspace` after any code change
- Respect the CLAD dependency graph: `khora-core` → `khora-data` / `khora-control` → `khora-lanes` → `khora-agents` → `khora-infra` → `khora-sdk`
- Use the engine's own math types (`khora_core::math::{Vec3, Mat4, Quat, LinearRgba}`) — never `glam` directly
- Follow existing naming conventions: `snake_case` for Rust, `PascalCase` for types, kebab-case for crate names
- Add `#[cfg(test)]` unit tests for any new public function
- Ensure all `unsafe` blocks have a `// SAFETY:` comment explaining correctness
- Keep GPU resource management through abstract IDs (`TextureId`, `BufferId`, `PipelineId`) — never raw wgpu handles in public APIs
- Use `log::info/warn/error` for logging — never `println!` or `eprintln!`
- Validate at system boundaries (user input, file I/O, GPU errors) — trust internal API contracts
- Write WGSL shaders for the wgpu backend — no GLSL or SPIR-V
- Route physics through `PhysicsProvider` trait and `StandardPhysicsLane` — never call Rapier directly from agents
- Route audio through `AudioDevice` trait and `SpatialMixingLane` — never call CPAL directly
- Route UI layout through `LayoutSystem` trait — never call Taffy directly from agents
- Use `AssetHandle<T>` for referencing loaded assets — never store raw data inline
- Implement serialization through the 3-strategy pattern (Definition/Recipe/Archetype) via `SerializationGoal`

## Must Never

- Introduce circular dependencies between crates
- Add `unwrap()` on fallible GPU operations (use `Result` / `?` / `map_err`)
- Bypass the Lane abstraction for hot-path work — all render/physics/audio pipelines go through `Lane::execute()`
- Commit code with Vulkan validation errors or wgpu warnings
- Use `std::thread::spawn` directly — concurrency goes through the DCC agent system
- Add dependencies without checking `deny.toml` advisories
- Modify the `khora-core` trait interfaces without updating all downstream implementations
- Use `Box<dyn Any>` downcasting as a substitute for proper trait design
- Store mutable global state — use `LaneContext` slots or ECS components

## Output Constraints

- Code changes must include the file path and exact line context
- Use markdown code blocks with language identifiers (```rust, ```wgsl, ```toml)
- Keep explanations under 5 sentences unless the user asks for detail
- When summarizing work, list files changed and tests affected

## Interaction Boundaries

- Only modify files within the `d:\Dev\KhoraEngine` workspace
- Do not push to git, create PRs, or modify CI without explicit permission
- Do not delete files or branches without confirmation
- Do not modify `.github/workflows/` without asking
