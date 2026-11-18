---
name: start-a-game
description: Starts a new Khora game — a hub project with scenes and Ergon scripts, and, when it needs Rust, its native crate on the SDK. Use when starting a game from scratch or adding native code to a project.
---

# Start a game

## Steps
1. **Create the project in the hub** (*Projects → New project*): name, parent folder, engine version.
   The hub writes `project.json`, `assets/{scenes,textures,meshes,audio,shaders,scripts}/` and a seed
   `assets/scripts/main.erg`; a project created there is trusted.
2. **Open it in the editor** (it opens on creation; or `khora-editor --project <path>`). The first open
   writes `assets/scenes/default.kscene` (a camera and a light).
3. **Build the scene** in the editor: Scene Tree → *Add*, Inspector for components, a material on every
   mesh (a mesh with none is not drawn). Save with `Ctrl+S`.
4. **Write the gameplay in Ergon** — [`../write-a-script/SKILL.md`](../write-a-script/SKILL.md) — and attach
   behaviors from the Inspector.
5. **Play** in the editor; **Stop** returns to the scene as authored.
6. **Native code, only when needed** (input bindings, game saves, backends, a family): the hub card's
   *Add Native Code* writes `Cargo.toml` (`khora-sdk = "<engine_version>"`, `anyhow`, `log`,
   `env_logger`) and `src/main.rs` calling `khora_sdk::run_default()`. Replace `main` with an
   `EngineApp + PhaseProvider` booted by `run_winit` — [`../../sdk-guide.md`](../../sdk-guide.md) §2 is the
   template (renderer, pipeline system, `open_game_assets`).

## Rules
- Gameplay in `assets/scripts/`; Rust on `khora-sdk` only (a family aside).
- Keep `#[global_allocator] static GLOBAL: SaaTrackingAllocator = SaaTrackingAllocator::new(std::alloc::System);`
  in `main.rs`: it feeds the engine's memory budget.
- No `unwrap()` on fallible I/O; no secrets in the project.

For gameplay detail see [`../../reference/gameplay.md`](../../reference/gameplay.md); scene/lighting see
[`../../reference/scene-design.md`](../../reference/scene-design.md). Verify: Play in the editor, or
`cargo run` the native binary — the window opens and the scene renders.
