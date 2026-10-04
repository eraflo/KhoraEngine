---
name: load-scene
description: Loads, saves, or serializes a scene or a game save in a Khora game via the SDK. Use when persisting world state, loading levels, saving/loading player progress, or working with prefabs.
---

# Load / save a scene, save the game

Two different documents, both `SceneFile`s:
- a **scene** (`.kscene`) — what was authored, loaded with `SerializationService`;
- a **game save** — only how play changed the world, taken **against a base scene**
  (`GameWorld::save_game` / `load_game`).

## Concepts
- `SerializationGoal` picks the **encoding**, never what is saved: `HumanReadableDebug` /
  `LongTermStability` → JSON text, `EditorInterchange` / `SmallestFileSize` → compact binary,
  `PortableBinary` → MessagePack, `FastestLoad` → a snapshot bound to this exact build (a cache — never
  the only copy you keep; a game save asking for it is written compact).
- Loads are **atomic**: on error the world is left exactly as it was. On success you get a `LoadReport`
  listing what was adapted (defaulted / dropped / renamed fields, retired components…). Log or show it.
- Prefabs: an instance stays linked to its `.kprefab` (a compact scene file); loading needs a prefab
  source. The engine sets one on your `GameWorld` at boot when an `AssetService` is present.
- The **editor** (`cargo run -p khora-editor` in the engine repo) authors scenes and prefabs.

## Steps
1. **Load a level** (startup, transition) — bytes from your packed assets, then:
   ```rust
   let file = SceneFile::from_bytes(&bytes).map_err(|e| anyhow::anyhow!("{e:?}"))?;
   let report = SerializationService::new().load_world(&file, world.inner_world_mut())?;
   for entry in &report.entries { log::warn!("{entry}"); }
   ```
   `replace_world` instead of `load_world` to swap the whole world. A level holding prefab instances
   needs `SerializationService::with_prefabs(source)` — without a source its load fails.
2. **Save the game** against the level it started from:
   ```rust
   let save: SceneFile = world.save_game(level_id, &level_file, SerializationGoal::SmallestFileSize)?;
   std::fs::write(path, save.to_bytes())?;
   ```
3. **Load the game**: read the save and its base level, then `world.load_game(&save, &level_file)?`.
   The save is merged onto the level *as it is now* — a level fix you ship later reaches existing saves
   wherever play left the value alone.
4. **Spawn a prefab** at runtime: `khora_sdk::khora_data::scene::instantiate_prefab(world.inner_world_mut(),
   prefab_id, prefabs)` (linked instance), or `khora_sdk::instantiate_subtree(world.inner_world_mut(),
   &bytes, prefabs)`. Both take `prefabs: &dyn PrefabSource` and return the new root `EntityId`.
   `PrefabSource` (`khora_sdk::khora_data::scene`) has one method, `prefab(id) -> Result<SceneRecord, String>`;
   the engine's asset-backed source is not re-exported, so implement it over your asset loading (or pass
   `NoPrefabs` to `instantiate_subtree` when the bytes hold no linked instance).

## Rules
- Treat scene/save bytes as untrusted input — handle every `Result`, never `unwrap()`.
- Keep the base level a save was taken against; a save alone cannot be loaded.
- Reference assets by handle; don't inline raw data.
- Scripts run only while the engine mode is `Playing` (the default for a game); `OnLoad` fires once per
  instance restored from a save.

For shipping packed scenes, see [`../pack-and-ship/SKILL.md`](../pack-and-ship/SKILL.md). Verify by loading
the scene and confirming entities appear, then save → load and compare.
