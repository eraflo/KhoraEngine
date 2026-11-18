# Khora SDK — Rules (gamedev profile)

Hard constraints for building a game with the Khora Engine. Read before writing game code.

- Document — Khora SDK Rules v2.0
- Status — Authoritative

---

## 1 — Must always

- Write gameplay in **Ergon**, under `assets/scripts/` (the only folder compiled). Declare the game's
  components there (`component Health { … }`); attach a behavior by adding its component.
- Build a game's Rust against **`khora-sdk`** and its `prelude`. A native game implements
  `EngineApp + PhaseProvider` (`impl PhaseProvider for MyGame {}` is enough) and boots through
  `run_winit`, or calls `run_default()`.
- In the `run_winit` bootstrap closure, register the renderer (`WgpuRenderSystem` and its
  `graphics_device()`), the `Arc<dyn PipelineSystem>` (`WgpuPipelineSystem`), and the other backends
  the game uses; open the game's assets with `open_game_assets::<MyGame>`.
- Install the game's own runtime resources in `EngineApp::provide`; cache handles in `setup`
  (`update` receives no `Runtime`).
- Spawn through `GameWorld` / `Vessel` and the spawn helpers. Move with `update_transform`, or call
  `sync_global_transform` after `get_transform_mut`.
- Math via `khora_sdk::prelude::math` (`Vec3`, `Quaternion`, `LinearRgba`) — never raw `glam`.
- Log via `log::{info,warn,error}` (init `env_logger` in `main`) — never `println!`.
- Write save files with `khora_sdk::fs::write_atomic`; read untrusted files with `fs::read_bounded`.
- Verify: Play in the editor, or `cargo run` the native binary.

## 2 — Must never

- **Never depend on internal `khora-*` crates** for gameplay or app code. Reach what the SDK
  re-exports (`khora_sdk::khora_core`, `khora_sdk::khora_data` included). **One exception:** a new
  subsystem is a **family**, and its crate depends on `khora-core`, `khora-macros` and `khora-agents`
  directly ([`skills/add-a-family`](./skills/add-a-family/SKILL.md)).
- Never write an agent type. A subsystem is a family of lanes and one `register_agent!`; its agent is
  the engine's `LaneAgent`.
- Never `#[derive(Component)]` in a game: the derive works inside `khora-data` only. Declare components
  in Ergon.
- Never load a scene synchronously while frames run: `load_game` / `replace_world` belong in `setup`;
  during play use `GameWorld::file_load_game` + `take_load_outcome`.
- Never `unwrap()` on fallible I/O (asset loading, save files, device open). Handle the `Result`.
- Never embed secrets/keys in a game build or commit them; the pack signing key
  (`~/.khora/signing/pack.key`) never enters a project ([`security-privacy.md`](./security-privacy.md)).
- Never push to git or create PRs without explicit permission.

## 3 — Boundaries (do-not-touch)

- Don't edit the generated AI wrappers (`CLAUDE.md`, `AGENTS.md`, `GEMINI.md`, `.github/`, `.claude/`,
  `.cursor/`, `.gemini/`) — they regenerate from `.agent/gamedev/` via the installer. Edit the source here.
- Don't hand-edit what the engine writes: `.khora/` (asset registry), `dist/`, `target/`.
- Don't trust a project on the user's behalf: trusting is the user's choice, in the hub or the editor.

## 4 — Permission model

- **No prompt:** read files, `cargo build`/`cargo run` the game, Play in the editor.
- **Ask first:** installing dependencies, any `git` write op, deleting files, Build Game / shipping a build.
- **Never without explicit permission:** push to git, create PRs, trust a project.

## 5 — Design

- For any UI / HUD / menu / visual decision, use **`/impeccable`** (audit / critique / polish).

*End of rules.*
