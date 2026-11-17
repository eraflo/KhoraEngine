# Serialization

<p class="kp-lede">A letter written today must still make sense to whoever opens it in ten years, even if words have changed meaning by then. So it says what each thing is, by name, rather than counting on its reader to guess from where it sits on the page. Khora saves a scene the same way: every value under its name, so that a scene saved by one version of a game opens in the next, and anything that no longer fits is pointed out instead of silently lost.</p>

Saving and loading scenes as **records**: what a file holds, by name, in the
shape of the world's pages. This page explains why a file is shaped that way,
what happens when it was written by older code, how a load changes the world all
at once or not at all, and how the engine picks an encoding. For the steps, see
[How-to: save and load scenes](../how-to/save-and-load-scenes.md); for the bytes,
see [File formats](../reference/formats.md).

Two pages build on this one: [Scenes and game saves](./saves.md) — a save is the
game's difference from its scene — and [Prefabs](./prefabs.md) — an instance is a
link and its difference from the prefab.

---

## The whole path at a glance

A save and a load are the same path, walked in opposite directions. In the middle
sits the **scene record** (`SceneRecord`): the world's authored content, by name,
page by page — independent of how it is then written to bytes.

<div class="kp-figure-frame">

{{#include ../images/persistence/round-trip.svg}}

</div>

- **Capture** reads the live pages and keeps only what a save holds: the
  components an author or a tool wrote, each entity under its persistent id.
- **Encode / decode** turn the record into bytes and back. Three encodings exist;
  all of them carry exactly the same record. A fourth form, the **snapshot**, skips
  the record for speed — see below.
- **Prepare / commit** bring a record into a world: everything is checked first,
  then the pages are built — or nothing changes at all.

## Why records

A save outlives the code that wrote it. Components gain fields, lose fields and
change names; a project's scenes are written by one version of the engine and read
by the next. A save that recorded values by position — field after field, as the
type laid them out the day it was written — breaks the moment a type changes, and
every change then needs a hand-written migration.

So a save holds **records**: each component under the name it registers, each field
under its name. Reading a record matches names, not positions, and adapts what
differs:

<div class="kp-figure-frame">

{{#include ../images/persistence/name-resolution.svg}}

</div>

- a field the save predates takes its default;
- a field the code no longer has is dropped;
- a field, variant or component renamed in code is found under the old name its
  type lists as `formerly`;
- a number is read at another width when that loses nothing — widened, or
  narrowed only when the value fits exactly;
- a reference to an entity the save does not hold is cut.

None of that is an error, and none of it is silent: every adaptation is an entry in
the **load report** (`LoadReport`) the caller receives. Each entry
(`ReportEntry`) names the entity's persistent id, the component, the field's path,
and what happened (`ReportKind`):

<div class="kp-figure-frame">

{{#include ../images/persistence/load-report.svg}}

</div>

| `ReportKind` | Means |
|---|---|
| `Renamed { from }` | the save used an older name, listed in `formerly` |
| `Defaulted` | the save predates the field; it took its default |
| `Dropped` | the code no longer has the field, or it holds a value of another type; its value is gone |
| `Widened` | a number read at another width, without loss |
| `Retired` | a component type declared retired; skipped |
| `NotSaved` | a component the engine derives or keeps while running; skipped, rebuilt |
| `DeadReference` | a reference to an entity the file does not hold |
| `RemovedFromScene` | a game save changed an entity its scene no longer has |
| `Unusable { message }` | a value the component's check rejects; kept, and reported |
| `NotLoaded { reason }` | the load added nothing (see [Atomic loading](#atomic-loading)) |

What *is* an error is a component name nothing registers — a save holding data
the engine cannot place is refused, not trimmed. A component removed on purpose
is declared **retired**; a save holding it loads, skips it and says so. So is a
component the engine derives: a save holding one — written by a tool, or before
the component stopped being saved — skips it and rebuilds it.

## Pages, persistent ids, provenance

Khora's ECS stores entities in pages, one per component signature, columns of
values side by side (see [Data and the ECS](./ecs.md)). The page is the unit of
iteration, of compaction — and of serialization. A record keeps that shape: page
records, each with its rows' ids and one column of values per component; an
entity whose components span several domains has a row in one page record per
domain. Capture walks the live pages; load builds pages directly, row by row, so a
loaded entity lands in its final page at once — no entity migrates from page to
page while a scene loads. A save holds named values, not the memory of a page, so
a layout the data layer adapts at runtime is never written to disk.

<div class="kp-figure-frame">

{{#include ../images/persistence/page-to-record.svg}}

</div>

Entities are referenced by **persistent id**, not by their runtime index. An
authored entity's id is random and scene-scoped; an entity inside a prefab
instance has an id derived from the instance's root and its own id in the prefab
(see [Prefabs](./prefabs.md#identity-inside-an-instance)); any other entity draws
from the *created* namespace — and only when something needs its identity: a
save, a lookup. Spawning entities nobody saves draws no identity. A reference
inside a component — a `Parent`, a target — is saved as the persistent id it
names, and loading binds it back to the entity that id now belongs to.

<div class="kp-figure-frame">

{{#include ../images/persistence/persistent-id.svg}}

</div>

| | Runtime `EntityId` | `PersistentId` |
|---|---|---|
| What it is | an index and a generation into the entity store | 64 bits; the top bit says which namespace |
| Authored entity | changes on every load | random 63 bits, chosen once, kept by every save |
| Inside a prefab instance | changes on every load | derived: `within(instance root, id in the prefab)` |
| Created at runtime | changes on every load | the next free number in the *created* namespace, given the first time it is needed |
| A script system's entity | changes on every load | named: `PersistentId::named("system:Name")`, the same in every save |
| In a save | never | every row and every entity reference |

A reference to an entity the save does not hold — deleted, or outside a saved
subtree — loads as a reference to **nowhere**: one entity id per world, reserved
once, never alive and never handed to a spawn, so such a reference can never come to
name a live entity. The report says where each one was.

A component's **provenance** decides where it is written:

| Provenance | In a scene | In a game save | Examples |
|---|---|---|---|
| `Authored` | yes | when the game changed it | `Transform`, `Name`, a behavior (`Guard`) |
| `ToolAuthored` | yes | when the game changed it | `Parent`, `PrefabInstance` |
| `Derived` | no — rebuilt on load | no | `Children`, `GlobalTransform` |
| `Runtime` | no | only if `resumable` | a behavior's state companion `Guard#state`; a system's component and its companion (all resumable) |

Derived values and plain runtime state are rebuilt rather than stored; a
resumable component is what a [game save](./saves.md#what-a-save-keeps-that-a-scene-never-does)
needs to resume a game instead of restarting it.

## Atomic loading

Loading is two steps — after a first one for scenes that link to prefabs, which
are [expanded](./prefabs.md) from the prefabs as they are now. **Prepare** reads
the whole record, reserves the entities and stages every value, touching nothing
the world shows. **Commit** then places the staged rows. A file that fails at any
point of prepare leaves the world exactly as it was, and the caller gets the reason
(`LoadFailure`) with the report of what had been read so far. Replacing a scene
uses the same split: the new scene is staged before the old one is removed.

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../images/persistence/prepare-commit.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>Names and shapes.</strong> A record still linking prefabs is refused. Page by page, each column's length is checked and each component's name resolved; an unknown name refuses the load.</li>
<li><strong>The plan, checked.</strong> An entity recorded twice, a row whose entity is not listed, a component twice in a page, two pages of one domain for one entity: each refuses the load.</li>
<li><strong>Ids reserved.</strong> One entity id per saved entity, held for this load, not alive, handed to no one else.</li>
<li><strong>Values staged.</strong> Fields read by name, references bound to the reserved ids or to nowhere, every adaptation reported.</li>
<li><strong>Hierarchy read.</strong> The tree comes from each <code>Parent</code>; a cycle refuses the load. The result is a <code>Prepared</code> load.</li>
<li><strong>Commit.</strong> The file's declarations registered, each saved identity kept unless taken, every row placed in its final page, <code>Children</code> built: the entities and the report.</li>
<li><strong>Or nothing.</strong> A failure in prepare gives the ids back; the world is exactly as it was.</li>
</ol>
</figure>

`Prepared::commit(world, identity)` returns no error. `Identity::Keep` keeps each
saved identity unless another entity holds it; `Identity::Fresh` gives every
entity a new authored one. `Prepared::abandon` gives the
reserved ids back. A commit can still add nothing: a load prepared for another
world reports `NotLoaded`, and a load prepared before the world's components
changed is prepared again against them first — reporting `NotLoaded` if that
fails.

### Loading while frames run

A prepare needs no `World`. It reads a **`LoadCatalog`** — what a load needs to
know of the world's components, taken with `World::load_catalog()` — and holds its
ids through a **`ReservationHold`** (`World::hold_reservations()`), so it can run
on another thread while frames go on: `prepare_with(&catalog, hold, record)`, and
`prepare_snapshot_with` for a snapshot. The catalogue has a **generation** that
moves when a component is registered, declared, relaid out, moved, or given
another provenance; a commit compares it with the world's to know whether to
prepare again.

A tool or a game that loads a scene while frames run **files** it with the
**`SceneLoadHub`** (`khora-io/src/scene_loads.rs`):

- `file(world, kind, prefabs)` returns a `LoadTicket`. The `kind` is
  `LoadKind::Add` (beside what is there), `LoadKind::Replace` (instead of it) or
  `LoadKind::Game` (a game save over its scene), each from bytes at hand or
  fetched off the frame;
- the **`loading` family** prepares it off the frame; the **`scene_intake`**
  DataSystem commits it at a frame boundary;
- loads land in the order they were filed, and `take_outcome(ticket)` returns
  the report or the error; `cancel(ticket)` makes a load land nothing;
- at most 64 loads wait to be prepared; one filed past that fails at once.

The synchronous `load_world`, `replace_world` and `load_game` are for setup, tests
and scratch worlds; a running tool never loads a scene on the frame.

## Goals and encodings

A save has more than one consumer. The editor wants something small and fast; a
diff in version control wants text; an external tool wants a format it can read.
The developer states a **`SerializationGoal`** — the intent — and the engine maps it
to an **encoding**:

| Goal | Encoding |
|---|---|
| `HumanReadableDebug`, `LongTermStability` | Text — pretty-printed JSON |
| `EditorInterchange`, `SmallestFileSize` | Compact — Khora's binary, page-shaped and column-major |
| `PortableBinary` | MessagePack, fields by name |
| `FastestLoad` | Snapshot — positional, bound to the build's schema |

The three record encodings carry the same record, so stability does not depend on
which one wrote a file: the rules above hold for a compact save exactly as for a text
one. The compact encoding stores each component and field name once, in tables, and
refers to them by index — the names are there, they are just not repeated per row.

The **snapshot** (`KH_SNAPSHOT_V1`) is the exception, on purpose. It writes values
by position — no names, nothing to match — and pays for that speed with a
contract: every Rust component is listed with the fingerprint of its schema, and a
snapshot whose fingerprints are not the running build's is refused whole. A
component a script declared is listed with its domain and its fields' names, kinds,
types and defaults, and restored onto the live declaration by name. That is how
engines ship cooked data (Unity's type-tree hashes, Unreal's unversioned
properties): a fast, nameless body beside a stable, named source. A snapshot is
never a project's only copy.

`FastestLoad` falls back to a compact record where a snapshot cannot hold: a world
holding a component no fingerprint can guard is saved as a compact record, with a
warning, and `save_game` under `FastestLoad` writes a compact record. A snapshot is
taken before prefab instances are collapsed into links, so it holds them expanded.

## One service, one file format

Scene save and load is a **service**, not an agent: it has no levels to choose
between, so it sits on the same side of the line as asset loading.

| Call | Does |
|---|---|
| `save_world(world, goal)` | writes the world as a scene file |
| `load_world(file, world)` | brings a scene in beside what is there |
| `replace_world(file, world)` | swaps the world's contents for the scene's — atomically |
| `save_game(world, base_id, base, goal)` | writes how the running game differs from its scene |
| `load_game(world, save, base)` | the scene as it is now, with the save merged on top |
| `save_base(save)` | which scene a save was taken against |

A service made with `SerializationService::with_prefabs(source)` saves prefab
instances as links and reads the prefabs a scene links to. One made with `new()`
reads none: it saves instances expanded, keeping the ids that tie them to their
instance, and refuses a scene that links to a prefab. A prefab that cannot be read
is saved expanded too, and linked again once it is back. `save_world_with`,
`load_world_with` and `replace_world_with` take a prefab source per call. Every
load returns its report.

Every file is a fixed 49-byte header followed by the payload: a magic number
(`KHORASCN` for a scene, `KHORASAV` for a game save), the format version (one
byte, 2), the encoding's name (32 bytes, such as `KH_COMPACT_V2`), and the
payload's length (8 bytes). The header is what makes loading symmetric: the caller
never says how a file was written, and a save handed to the scene loader is refused
for what it is (`NotAScene`). An older format is refused (`OldFormat`), as is a
newer one (`NewerFormat`).

## How components serialize

Component serialization is generated, not hand-written. Deriving `Component` on a
type generates a serializable mirror, the conversions to and from the live type,
and a self-registration so loading finds the type by name with no hand-maintained
list. Fields marked `#[component(skip)]` — GPU handles, runtime caches — are left out
of the mirror and take their default on load; the engine rebuilds what they hold,
typically through the asset resolver and the residencies. A type renamed in code
keeps its saves readable with `#[component(formerly = "OldName")]`; a field, with
`#[component(formerly = "old_name")]`.

A component a script declares (`component Health { … }` in Ergon) has no Rust type
and no registration, so a record holding one **describes it**: beside its pages,
the record lists each such component's name, domain, whether it is resumable, and
its fields. Its values are written field by field, by name, each field the tagged
value it is (`"current": {"Int": 42}`): a cell says what it holds whatever
declaration reads it, so records saved under two declarations of one component — a
prefab saved before its scene, a game saved before its scene was — still merge. A
game save is compared under the declarations of the world it is taken from. Each
declared field also records its Ergon type — a struct with its fields — so a value
of another type (text saved for what is now an `int?`) is dropped, reported, never
read into it. A world that never heard of it — a scene opened before any script
ran, a scene of a project that no longer declares it — registers it from the file
when the load commits; a world declaring it differently reads the file's fields
onto its own by name, and the load report says what it adapted. A name neither
registered, described, nor retired still fails the whole load.

## Play-mode snapshots

Pressing Play snapshots the world: a `save_world` into memory under `FastestLoad`,
by a service with no prefab source — the same build writes and reads it, so the
positional form is safe. Pressing Stop files the snapshot with the `SceneLoadHub`
as a `Replace`; it is prepared off the frame and lands at a later frame's boundary,
atomically, every persistent id kept and every runtime `EntityId` fresh (so the
editor clears its selection). Play is refused until it has landed. Stop is a fresh
start, not a restored game: scripts run `OnSpawn` again on the next Play, never
`OnLoad`.

**Physics state is not kept** across a snapshot: `BodyMotion` is runtime state,
so on restore each body starts again from its authored `RigidBody` — its initial
velocity — and the physics engine rebuilds from the component data.

## Under the hood

**Notation.** Write *W* for a world's observable state — its live entities, their
components and their persistent ids — and *load(f, W)* for the state after loading
the file *f*.

**Theorem (atomicity).** For every file *f*, either *load(f, W) = W*, or
*load(f, W)* holds every entity *f* records, with every component the report does
not list as skipped.

*Proof sketch.* Prepare writes nothing observable: its ids are reserved — held,
never alive, invisible to every query and never handed to a spawn — and its values
are staged outside the pages. A failure in prepare drops the hold, which returns
the ids, so *W* is unchanged. Commit has no failing step once the load is current
for the world: every check that could fail ran in prepare, against the same
components (the catalogue's generation proves it; otherwise the load is prepared
again, and a failure there adds nothing and reports `NotLoaded`). It brings every
reserved id to life and places every staged row. ∎

**Identity.** Under `Identity::Keep`, the saved id *p* of an entity is kept iff no
entity of *W* holds *p* before the commit; the others take a fresh id of their
namespace. So two loads of one scene into one world keep the first load's ids, and
the second load's entities get fresh ones: no persistent id ever names two live
entities.

**References.** A saved reference to *p* is bound to the entity reserved for *p*
when *f* holds *p*, and to the world's one *nowhere* id otherwise. *Nowhere* is
reserved once per world and never alive, so a reference cut at load never comes to
name an entity spawned later.

## Next steps

- [How-to: save and load scenes](../how-to/save-and-load-scenes.md) — save with a
  goal and load a scene with the real service API.
- [Scenes and game saves](./saves.md) — the game's difference from its scene,
  and the three-way merge that loads it.
- [Prefabs](./prefabs.md) — links, overrides, derived ids.
- [File formats](../reference/formats.md) — the header, the record, the encodings.
- [Data and the ECS](./ecs.md) — pages, and the component model the mirror is
  generated from.
- [Assets](./assets.md) — how scene references to assets resolve by uuid.
