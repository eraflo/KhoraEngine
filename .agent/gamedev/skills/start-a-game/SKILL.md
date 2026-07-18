---
name: start-a-game
description: Scaffolds a new game project on the Khora SDK. Use when starting a game from scratch — sets up the binary crate, the EngineApp type, main(), and the backend bootstrap.
---

# Start a game

Create a Cargo **binary** crate that depends on `khora-sdk`.

## Steps
1. `Cargo.toml` — add `khora-sdk`, `anyhow`, `env_logger`, `log` (and `winit` if you downcast the event loop).
2. `src/main.rs`:
   - `#[global_allocator] static GLOBAL: SaaTrackingAllocator = SaaTrackingAllocator::new(std::alloc::System);`
   - A game type implementing `EngineApp + AgentProvider + PhaseProvider` (see [`../../sdk-guide.md`](../../sdk-guide.md) §1).
   - `main()` initializes `env_logger`, then `run_winit::<WinitWindowProvider, MyGame>(bootstrap)`.
   - In the bootstrap closure, register the backends you need (render required; physics/audio/text optional) — §4 of the guide.
3. In `setup`, spawn a camera + a light + some geometry so you see something.
4. `cargo run` — confirm the window opens and the scene renders.

## Rules
- SDK-only; `prelude::*`; no internal `khora-*` deps; no `unwrap()` on backend init you can recover from; no secrets.

For gameplay detail see [`../../reference/gameplay.md`](../../reference/gameplay.md); scene/lighting see
[`../../reference/scene-design.md`](../../reference/scene-design.md). The `sandbox` example is the working reference.
