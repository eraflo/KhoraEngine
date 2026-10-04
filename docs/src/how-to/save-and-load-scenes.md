# Save and load scenes

This guide shows you how to write the world to a `.kscene` file and load it back,
how to save and restore a player's game against that scene, and how to keep old
files readable as your components change. For *why* it works this way, see
[Serialization](../concepts/serialization.md) and
[Scenes and game saves](../concepts/saves.md).

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

You pick a **goal** (your intent), and the service picks the matching encoding.
The header records which one, so loading never asks:

| Goal | Encoding | Pick it for |
|---|---|---|
| `HumanReadableDebug`, `LongTermStability` | JSON text | files in version control, files you read |
| `EditorInterchange`, `SmallestFileSize` | Khora's compact binary | the editor's own scenes, shipped levels |
| `PortableBinary` | MessagePack | files another tool reads |
| `FastestLoad` | snapshot | data the same build writes and reads |

The first three save components and fields **by name**, so the file stays
readable as your component types change.

`FastestLoad` is different: it writes a **snapshot**, positional and about nine times
faster to load, but readable only by a build whose component schemas are the same —
change a component and the snapshot is refused. Keep a named save beside it, and use
snapshots for what the same build writes and reads (a shipped build's data, an
in-memory checkpoint).

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

## Save the player's game

A game save is not a scene: it holds how the running game differs from the scene
it started from, and names that scene. Keep the scene's asset id and file at
hand, and save against them:

```rust,ignore
use khora_sdk::SerializationGoal;

match world.save_game(level_id, &level_file, SerializationGoal::SmallestFileSize) {
    Ok(save) => {
        if let Err(e) = std::fs::write("saves/slot_1.save", save.to_bytes()) {
            log::error!("failed to write save: {e}");
        }
    }
    Err(e) => log::error!("failed to save the game: {e:?}"),
}
```

To restore it, load the scene file the save names, then the save on top:

```rust,ignore
let save = SceneFile::from_bytes(&save_bytes)?;
let level_id = service.save_base(&save)?; // which scene to open
let level_file = /* read the scene with that id */;

match world.load_game(&save, &level_file) {
    Ok(report) => for entry in &report.entries { log::info!("restore: {entry}") },
    Err(e) => log::error!("failed to restore the game: {e}"),
}
```

The scene is read **as it is now**: if you patched the level since the save was
taken, every value the player's game did not touch takes your patch. Scripts are
restored part-way through what they were doing, and run `OnLoad` before anything
else. Like every load, `load_game` either succeeds or leaves the world exactly as
it was.

There is no file convention for saves — `save_game` hands you a `SceneFile`
whose header reads `KHORASAV`, and where it goes is the game's decision. A save
never uses the snapshot encoding: asked for `FastestLoad`, it is written
compactly.

## Scenes that link to prefabs

A scene the editor saved holds its [prefab instances](../concepts/prefabs.md) as
links. Loading it needs the prefabs: a shipped game's world has them, but a
service you build yourself must be given a source with
`SerializationService::with_prefabs`. A service from `new()` refuses a scene
that links to a prefab — see
[Work with prefabs](./work-with-prefabs.md#load-scenes-that-link-to-prefabs-from-code).

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

A component type you delete on purpose is declared **retired**, so old saves
holding it load, skip it, and say so in the report:

```rust
inventory::submit! {
    khora_data::scene::RetiredComponent { name: "Glow" }
}
```

Any other component name the engine does not know makes the load fail with an
error naming it — the world is left as it was. Fields need nothing: one the
code gained takes its default, one it lost is dropped, both reported.

## Play-mode snapshot note

The editor takes a `FastestLoad` snapshot of the world on **Play** and restores it
on **Stop**, so gameplay never mutates the authored scene. That is not a game
save: Stop throws the game away, and the next Play starts fresh. Physics state is
**not** preserved across the snapshot — bodies rebuild from component data, and
velocities and contacts reset to defaults.

## Expected result

`save_world` produces a `.kscene` file (JSON text under the readable goals, binary
otherwise) beginning with the `KHORASCN` magic; loading it into a fresh world reproduces
the same entities, with the same persistent ids, and their saved components. A corrupt,
truncated or outdated file is reported through the `Result` rather than panicking.

## Related

- [Serialization](../concepts/serialization.md) — records, pages, atomic loading.
- [Scenes and game saves](../concepts/saves.md) — what a save holds and how it merges.
- [Work with prefabs](./work-with-prefabs.md) — make, place, override, apply.
- [File formats](../reference/formats.md) — the header, the three encodings and the snapshot.
- [Spawn entities and move them](./spawn-and-transform.md) — build the scene you save.
- [`SerializationService` reference](../reference/sdk.md) — goals and error types.
