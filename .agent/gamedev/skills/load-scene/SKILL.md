---
name: load-scene
description: Loads or saves a scene, or a player's game save, from a Khora game's Rust through the SDK. Use when persisting player progress, loading a level from code, or working with scenes that link prefabs.
---

# Load / save a scene, save the game

Two documents, both `SceneFile`s:
- a **scene** (`.kscene`, magic `KHORASCN`) — the whole world as authored, read and written by
  `SerializationService`. The editor writes them; a shipped game loads the scene `runtime.json` names.
- a **game save** (magic `KHORASAV`) — only how play changed the world, taken **against a base scene**
  (`GameWorld::save_game` / `load_game` / `file_load_game`).

## Concepts
- `SerializationGoal` picks the **encoding**, never the content: `HumanReadableDebug` /
  `LongTermStability` → JSON text, `EditorInterchange` / `SmallestFileSize` → compact binary,
  `PortableBinary` → MessagePack, `FastestLoad` → a snapshot only this build reads (a cache, never the
  only copy; a game save asking for it is written compact).
- Every load is **atomic**: an error leaves the world exactly as it was; success returns a `LoadReport`
  whose `entries` say what was adapted (defaulted, dropped, renamed, retired…). Log them.
- **While frames run, file the load**: `world.file_load_game(&save, &level)` returns a `LoadTicket`; the
  load is prepared off the frame and lands at a frame boundary; `world.take_load_outcome(ticket)`
  returns it once landed. The synchronous `load_game`, `load_world` and `replace_world` are for `setup`.
- **Prefabs**: a scene from the editor holds prefab instances as links. The world the engine hands to
  `setup` has a prefab source when an `AssetService` was opened before boot (`open_game_assets`); a
  `SerializationService` you build needs `SerializationService::with_prefabs(source)` — `new()` refuses
  a scene that links one.

## Steps
1. **Save the game** against the level it started from — [`../../sdk-guide.md`](../../sdk-guide.md) §10 has
   `save`, `file_load` and `landed`:
   `world.save_game(level_id, &level, SerializationGoal::SmallestFileSize)?` then
   `khora_sdk::fs::write_atomic(path, &save.to_bytes())?`. The game picks the folder; the engine names none.
2. **Load the game**: read the save with `fs::read_bounded`, find its level with
   `SerializationService::new().save_base(&save)?`, read that level, then `file_load_game` during play
   (or `load_game` in `setup`). The save merges onto the level **as it is now**.
3. **Write a scene** from code: `SerializationService::new().save_world(world.inner_world(), goal)?`.
4. **Load a scene** in `setup`:
   ```rust
   use khora_sdk::{SceneFile, SerializationService};

   let scene = SceneFile::from_bytes(&bytes).map_err(|e| anyhow::anyhow!("{e:?}"))?;
   let report = SerializationService::new().replace_world(&scene, world.inner_world_mut())?;
   for entry in &report.entries {
       log::warn!("{entry}");
   }
   ```
5. **Spawn a prefab at runtime**: from a script, `Spawn(prefab, …, at: p)` (preferred); from Rust,
   `khora_sdk::khora_data::scene::instantiate_prefab(world.inner_world_mut(), prefab_id, &source)` or
   `khora_sdk::instantiate_subtree(world.inner_world_mut(), &bytes, &source)`, each taking a
   `&dyn PrefabSource` (`khora_sdk::khora_data::scene::{PrefabSource, NoPrefabs}`).

## Rules
- Save and scene bytes are untrusted input: bounded reads, every `Result` handled, no `unwrap()`.
- Keep the base level a save names; a save alone cannot be loaded.
- Scripts run `OnLoad` once per instance a game save restored, before anything else.
- Reference assets by handle; never inline raw data.

Verify: load, see the entities; then save → load and compare.
