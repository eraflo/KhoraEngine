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

You pick a **goal** (your intent), and the service picks the matching encoding:
`HumanReadableDebug` / `LongTermStability` write JSON text (readable, diffable);
`EditorInterchange` / `SmallestFileSize` / `FastestLoad` write Khora's compact binary;
`PortableBinary` writes MessagePack. The `.kscene` header records which one, so
loading is symmetric. Whichever you pick, components are saved by name and fields by
name, so the file stays readable as your component types change.

## Load the world

Parse the bytes into a `SceneFile`, then load it. `replace_world` swaps the world's
contents for the scene's; `load_world` brings the scene in beside what is already
there. Either way the load is atomic — a file that cannot be loaded leaves the world
untouched — and returns a report of what it adapted:

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

let service = SerializationService::new();
match service.replace_world(&scene, world.inner_world_mut()) {
    Ok(report) => {
        for entry in &report.entries {
            log::warn!("level_01: {entry}");
        }
    }
    Err(e) => log::error!("failed to load scene: {e}"),
}
```

The report lists every place the save differed from today's code — a field that took
its default, one that was dropped, one read from its old name. An empty report means
the scene was read exactly as written.

## Keep old saves readable when you rename

Rename a component or a field and say what it was called; saves written before the
rename keep loading:

```rust
#[derive(Component, Clone, Default)]
#[component(formerly = "Health")]
pub struct Vitality {
    #[component(formerly = "hp")]
    pub points: f32,
}
```

A component type you delete on purpose is declared retired, so old saves holding it
load and skip it. Any other component name the engine does not know makes the load
fail with an error naming it — the world is left as it was.

## Play-mode snapshot note

The editor uses `EditorInterchange` to snapshot the world on **Play** and restore it on
**Stop**, so gameplay never mutates the authored scene. Physics state is **not** preserved
across the snapshot — bodies rebuild from component data, and velocities and contacts reset
to defaults.

## Expected result

`save_world` produces a `.kscene` file (JSON text under the readable goals, binary
otherwise) beginning with the `KHORASCN` magic; loading it into a fresh world reproduces
the same entities, with the same persistent ids, and their saved components. A corrupt,
truncated or outdated file is reported through the `Result` rather than panicking.

## Related

- [Serialization](../concepts/serialization.md) — records, pages, atomic loading.
- [File formats](../reference/formats.md) — the header and the three encodings.
- [Spawn entities and move them](./spawn-and-transform.md) — build the scene you save.
- [`SerializationService` reference](../reference/sdk.md) — goals and error types.
