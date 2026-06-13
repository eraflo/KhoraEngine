# Save and load scenes

This guide shows you how to serialize the world to a `.kscene` file and load it back.

> **Prerequisites.** You have a populated `GameWorld`
> ([Spawn entities and move them](./spawn-and-transform.md)).

## Save the world

`SerializationService` saves a `World` to an in-memory `SceneFile`; call
`scene.to_bytes()` to get the bytes and write them to disk. You serialize the
`GameWorld`'s inner `World` via `world.inner_world()`. Never `unwrap()` the I/O —
handle the `Result`.

```rust
use khora_sdk::{SceneFile, SerializationGoal, SerializationService};

let service = SerializationService::new();
match service.save_world(world.inner_world(), SerializationGoal::HumanReadableDebug) {
    Ok(scene) => {
        if let Err(e) = std::fs::write("levels/level_01.kscene", scene.to_bytes()) {
            log::error!("failed to write scene: {e}");
        }
    }
    Err(e) => log::error!("failed to serialize scene: {e:?}"),
}
```

You pick a **goal** (your intent), and the service picks the matching strategy:
`HumanReadableDebug` / `LongTermStability` produce RON (readable, diffable);
`EditorInterchange` / `SmallestFileSize` produce compact binary; `FastestLoad` produces
the archetype layout; `PortableBinary` produces MessagePack. Choosing a goal is your
decision; choosing the strategy is the engine's — the `.kscene` header records which one,
so loading is symmetric.

## Load the world

Parse the bytes into a `SceneFile`, then populate the world's inner `World`. Despawn the
existing entities first so you replace rather than merge:

```rust
use khora_sdk::{SceneFile, SerializationService};

let bytes = match std::fs::read("levels/level_01.kscene") {
    Ok(b) => b,
    Err(e) => { log::error!("read failed: {e}"); return; }
};
let scene = match SceneFile::from_bytes(&bytes) {
    Ok(f) => f,
    Err(e) => { log::error!("invalid scene file: {e:?}"); return; }
};

// Replace the current scene.
let existing: Vec<_> = world.iter_entities().collect();
for entity in existing {
    world.despawn(entity);
}

let service = SerializationService::new();
if let Err(e) = service.load_world(&scene, world.inner_world_mut()) {
    log::error!("failed to load scene: {e:?}");
}
```

`load_world` reads the strategy id from the header and dispatches the right decoder; the
goal you saved with does not need to be repeated.

## The three strategies, in one sentence

Definition serializes to human-readable RON, Recipe to a compact binary command stream,
and Archetype to a near-`memcpy` page layout — picked for you by the `SerializationGoal`
you pass. See [Serialization](../concepts/serialization.md) for the file format and the
goal → strategy mapping.

## Play-mode snapshot note

The editor uses `EditorInterchange` to snapshot the world on **Play** and restore it on
**Stop**, so gameplay never mutates the authored scene. Physics state is **not** preserved
across the snapshot — bodies rebuild from component data, and velocities and contacts reset
to defaults.

## Expected result

`save_world` produces a `.kscene` file (RON text under the debug goals, binary otherwise)
beginning with the `KHORASCN` magic; loading it into a fresh world reproduces the same
entities and components. A corrupt or truncated file is reported through the `Result`
rather than panicking.

## Related

- [Serialization](../concepts/serialization.md) — strategies, `.kscene` format, migrations.
- [Spawn entities and move them](./spawn-and-transform.md) — build the scene you save.
- [`SerializationService` reference](../reference/sdk.md) — goals and error types.
