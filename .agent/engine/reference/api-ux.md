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
- `EngineApp::initial_mode()` (`traits.rs`, default `EngineMode::Playing`) seeds `SharedEngineMode`;
  `EngineMode`, `SharedEngineMode`, `DccService` are re-exported. Scripts run only while `Playing`.
- `run_default` loads the packed default scene with `SerializationService::with_prefabs(AssetPrefabs)`
  and logs each `LoadReport` entry.

## Principles
- **The SDK is a façade** — the Scheduler, `BudgetChannel`, `EnginePlugin`, and all `khora-*` internals stay
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
