# Scenes and game saves

A game has two kinds of file, written by two different people. The **scene** is
what the author made in the editor. The **save** is what a player's game did to
it. Khora keeps them apart on purpose: a save never repeats the scene, it records
how the running world differs from it. This page explains that split, how a save
is loaded back, and why the result survives the author editing the scene
afterwards.

For the steps, see [How-to: save and load scenes](../how-to/save-and-load-scenes.md).
For how either file reaches the disk — records, names, encodings — see
[Serialization](./serialization.md).

---

## Two files, one world

<div class="kp-figure-frame">

{{#include ../images/persistence/scene-and-save.svg}}

</div>

| | Scene | Game save |
|---|---|---|
| Written by | the editor, from what the author placed | the game, while it runs |
| Holds | every authored entity and component | only what differs from its scene |
| Header magic | `KHORASCN` | `KHORASAV` |
| Names its scene | — | yes, by the scene's asset id |
| Runtime state | never | what the engine needs to resume — a script's fields, a body part-way through |
| Loaded with | `load_world` / `replace_world` | `load_game(save, scene)` |

The first consequence is size: a save in a level where the player moved three
things holds three things. The second is the one that matters — a save made
before the author's last patch still loads into the patched scene, and gets the
patch.

## What a save holds

A save is a `SaveRecord`, page-shaped like a scene, kept against the scene it
was taken from:

| Field | What it records |
|---|---|
| `base` | the scene the save was taken against, by asset id |
| `order` | every entity of the saved world, tree by tree, each parent's children in order — so siblings keep the order play left them in |
| `created` | the entities the game made; their ids come from the [created namespace](./serialization.md#pages-persistent-ids-provenance) |
| `destroyed` | the scene's entities the game destroyed |
| `removed` | the components the game removed from the scene's entities, **named** — a component the author adds to the scene later is not one the game removed |
| `changes` | every entity that differs from the scene, with the components that differ, **whole** |
| `before` | the scene's value, when the save was taken, of every component in `changes` |

A changed component is stored whole rather than as a patch of the fields that
moved. A patch cannot be read back reliably once the type changes: a field it
lacks is not a field the game left alone, and a name it carries may be one the
type has since given up. Whole values go through the same by-name reader as
everything else, so a component renamed or reshaped since the save is read as
today's type first, and compared second.

## Loading a save: a three-way merge

Loading takes the scene **as it is now**, and the save. For every component the
save changed, it reads three values through today's type — the scene's value
when the save was taken (`before`), the game's value (`changes`), and the
scene's value now — and decides field by field:

<div class="kp-figure-frame">

{{#include ../images/persistence/three-way-merge.svg}}

</div>

- a field the **game changed** loads as the game left it — even if the author
  changed it too: the player's game is what the save is for;
- a field the **game left alone** follows the scene as it is now, so the
  author's later edit reaches every save;
- an entity the save **created** is added; one it **destroyed** stays gone;
  a component it **removed** stays removed.

Two cases cannot be merged, and the load says so in its report instead of
failing:

- the game changed an entity the scene **no longer has** — the scene's removal
  wins, the change is dropped, and the report has a `RemovedFromScene` entry;
- a value names an entity neither the scene nor the save holds — it loads as a
  reference to nowhere, reported as a `DeadReference`.

The whole load is [atomic](./serialization.md#atomic-loading): if anything
cannot be read, the world is exactly as it was before the call.

## What a save keeps that a scene never does

A component says what it is through its **provenance**. Authored and
tool-authored components are a scene's content. Runtime components — what the
engine computes or keeps while running — are never written to a scene. A few of
them are needed to *resume* a game rather than restart it, and declare
themselves **resumable**:

```rust
#[derive(Component)]
#[component(domain = Script, provenance = Runtime, resumable)]
pub struct ScriptState { /* fields, countdowns, the body under way, lifecycle */ }
```

A save keeps every resumable component of every entity it holds. A script's
fields are merged like everything else: its state records, beside each field, the
authored value it started from — the scene's override or the declared default —
so a field the game never changed takes the author's edit made since, and a field
the game changed keeps the game's value. That is how a
guard saved half-way through an attack loads half-way through it: its
[script state](./scripting.md#saved-mid-sentence) carries the guard's fields,
its timers' remaining time, the machine stopped at its `await`, and the facts
of its life — whether it has spawned, whether it faulted and under which code.

## Lifecycle across a save

Because those facts are recorded, not guessed, each lifecycle member runs
exactly when its name says:

| Moment | `OnSpawn` | `OnLoad` |
|---|---|---|
| A scene starts playing — Play in the editor, a fresh level | runs, once per entity | — |
| A game save is restored | — the save remembers it already ran | runs, before anything else |
| An entity spawned during play | runs | — |
| Stop in the editor, then Play again | runs again — a fresh start, not a restore | — |

`OnLoad` is the place for what a script must rebuild after a restore — a
cached handle, a sound that should resume. A body the save caught part-way
waits behind it and resumes once `OnLoad` is done.

## Pressing Play is not saving a game

The editor's Play button does not take a game save. It takes a **snapshot** of
the scene — the fastest encoding there is, bound to the build that wrote it —
and Stop puts it back. A snapshot holds the scene, not a game: Stop throws
away what the game did, and the next Play starts fresh, with `OnSpawn`, never
`OnLoad`. See [the editor's play modes](../reference/editor.md#04--play-mode).

A game save, by contrast, never uses the snapshot encoding even when asked for
the fastest load: it holds state the engine wrote while running, which a
snapshot refuses, so it is written compactly instead.

## In code

```rust,ignore
// Save the running game against the scene it was started from.
let save = world.save_game(scene_id, &scene_file, SerializationGoal::SmallestFileSize)?;

// Later — even after the author patched the scene — load it back.
let report = world.load_game(&save, &scene_file)?;
for entry in &report.entries {
    log::info!("{entry}");
}
```

`SerializationService::save_base(&save)` reads which scene a save was taken
against, so a loader can find the right scene file before loading.

## Next steps

- [How-to: save and load scenes](../how-to/save-and-load-scenes.md) — scenes,
  game saves and prefabs with the real API.
- [Serialization](./serialization.md) — records by name, persistent ids, the
  load report, encodings.
- [Prefabs](./prefabs.md) — the other kind of difference a file can hold.
- [Scripting](./scripting.md) — what a script's saved state is, and how a body
  resumes after an edit.
