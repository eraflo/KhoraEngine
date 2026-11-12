# API / UX (SDK surface) — reference

Domain knowledge for the public `khora-sdk` surface. Consulted during Research (dispatch the `codebase-*`
subagents to apply it to concrete files).

## Scope
The `khora-sdk` surface that game developers touch: `EngineApp`, `GameWorld` (incl. game saves),
`Vessel`, `run_winit`/`run_default`, the `prelude`, and the re-export discipline that keeps internals
private.

## Key files
- SDK: `crates/khora-sdk/src/` (`lib.rs`, `engine/`, `game_world.rs`, `vessel.rs`, `traits.rs`, `run_default.rs`).
- I/O surface: `khora-io` (asset/serialization).

## Persistence surface
- `GameWorld` (`game_world.rs`): `save_game(base_id, &base, goal) -> SceneFile`,
  `load_game(&save, &base) -> LoadReport` (atomic: on error the world is unchanged), and
  `set_prefabs(Arc<dyn PrefabSource + Send + Sync>)` — the bootstrap already sets `AssetPrefabs` when an
  `AssetService` is present (`engine/bootstrap.rs`). A save is always taken against a base scene.
- Re-exports (`lib.rs`): `SceneFile`, `SerializationGoal`, `LoadReport`, `SerializationService`,
  `SerializationServiceError`, `serialize_subtree` / `instantiate_subtree`, `ComponentRegistration`;
  everything else (`PrefabSource`, `instantiate_prefab`, `ReportKind`) via the `khora_sdk::khora_data`
  re-export. `AssetPrefabs` (`khora-io`) is not reachable from the SDK — the bootstrap installs it.
- No engine mode: `khora_sdk::Activation` (re-export of `khora_core::agent::Activation`) is published as
  `Arc<Activation>` before `setup`; every family starts on, `activate`/`deactivate` are requests settled
  at the next frame boundary (`before_frame` is the app's last place before it). `DccService` is
  re-exported too. Scripts run only while `khora_core::script::SCRIPT` is on.
- Startup is engine first: an app files its heavy work by ticket (`IndexHub`, `SceneLoadHub`,
  `scripts::mount`) and waits on it through `Readiness` (`startup_ready` once, 60 s timeout). A mount filed in
  the bootstrap closure is compiled by the boot, so `setup` can attach script behaviours by name.
- `run_default` loads its game by ticket, without a film:
  - loose `assets/` are indexed by an `IndexHub` `Open`;
  - the default scene is filed with the `SceneLoadHub` (prefabs from `AssetPrefabs`), and each
    `LoadReport` entry is logged when it lands;
  - its scripts are mounted in `setup`.

## Principles
- **The SDK is a façade** — the Scheduler, `BudgetChannel` and all `khora-*` internals stay
  hidden. Game code sees only the public API. Add a re-export only when game code genuinely needs the type.
- Prefer ergonomic builders (`Vessel::at(world, pos).with_component(..).build()`) and a clean `prelude`.
- `#[non_exhaustive]` on public enums/structs likely to grow; newtype IDs, no bare integers.
- `#![warn(missing_docs)]` is on — every public item needs rustdoc with a compiling `# Examples` block.
- Keep the gamedev profile ([`../../gamedev/`](../../gamedev/)) in sync: a public-API change should be
  reflected in the gamedev `sdk-guide.md` and skills.

Use codegraph to check what each re-export pulls in.

## Skills
- [`add-a-component`](../skills/add-a-component/SKILL.md) — when surfacing a new component to game code.
- [`build-and-test`](../skills/build-and-test/SKILL.md) — verify (`cargo build` + `cargo test --doc` for examples).
