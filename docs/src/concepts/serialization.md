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
reserved ids back. A commit can still add nothing, for one of two reasons, each a
`NotLoaded` entry: the load was prepared for another world (its ids were held
from another world's reserver), or the world's components changed since it was
prepared and preparing it again against them fails.

**A load keeps what it was read from.** Every prepared load shares its source —
the record, or the snapshot file — behind an `Arc`, never a copy: `prepare`,
`prepare_game` and `prepare_snapshot` take it by value or already shared, as
their off-the-frame forms do (below). When the world's components changed between
prepare and commit, the commit prepares the load again from that source, the way
it was first prepared — a scene, a game or a snapshot — and it lands as a load
prepared at that moment would. Where it was prepared, with the world or off the
frame, makes no difference. `apply` and a prefab instance prepare and commit
under one borrow of the world: nothing can change the components in between, so
they keep no source.

**The ids a load holds.** Prepare reserves one entity id per saved entity, held in
a `ReservationHold` until the commit (see
[Data and the ECS — reserved ids](./ecs.md#the-storage-model)). It takes the
slots despawns freed first, then new ones. A load abandoned, failed or cancelled
drops its hold, and its ids go back as they were: they never lived, so the next
reservation or spawn takes them as they are. A committed id is never handed out
again; a slot freed later by a despawn comes back with its generation moved on.

### What a composed load reports

A load can be several reads in one: a scene's prefab links expanded, then a game
save merged over it. Each read reports, and the reports are joined so that each
entry says something about what finally loads, once:

- an entry about an entity, or an entity's component, that the final record does
  not hold is left out — a prefab's member the instance deleted, a component the
  save removed;
- a `Retired` entry stays if its entity loads, and `RemovedFromScene` always
  stays: it is about what does not load;
- an entry naming no entity stays;
- an exact duplicate is kept once.

`DeadReference` and `Unusable` are reported by the final prepare only, where the
value lands, not by the reads before it. A field's path is the path inside the
value (`translation.x`, `lights[2]`); an entity inside a prefab instance is named
by its instance id.

### Loading while frames run

A prepare needs no `World`. It reads a **`LoadCatalog`** — what a load needs to
know of the world's components, taken with `World::load_catalog()` — and holds its
ids through a **`ReservationHold`** (`World::hold_reservations()`), so it can run
on another thread while frames go on: `prepare_with(&catalog, hold, record)`,
`prepare_game_with` for a game and `prepare_snapshot_with` for a snapshot. The
catalogue has a **generation** that moves when a component is registered,
declared, relaid out, moved, or given another provenance; a commit compares the
load's with the world's to know whether to prepare it again, as it does for every
load.

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

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../images/persistence/filed-loads.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>Filed.</strong> Three loads are filed: tickets 1, 2 and 3. Each takes a hold for its ids and the world's catalogue as it is now, and waits for the <code>loading</code> family.</li>
<li><strong>Prepared out of turn.</strong> Ticket 2 is prepared first. It does not land: ticket 1, filed before it, is not ready, and loads land in the order they were filed.</li>
<li><strong>In turn.</strong> Ticket 1 is prepared. At the next frame boundary, <code>scene_intake</code> commits 1, then 2: the leading run of prepared loads.</li>
<li><strong>Prepared against old components.</strong> A script reload changed a component before ticket 3 landed. Its lease fails: it is given back to the <code>loading</code> family, ahead of loads filed since, with the same ids, and prepared again.</li>
<li><strong>Cancelled.</strong> Had ticket 3 been cancelled instead, it would land nothing, its ids would go back, and its outcome would never be filed.</li>
</ol>
</figure>

What holds while frames run:

- **Order.** Each boundary commits the leading run of prepared loads, in ticket
  order, and stops at the first load not prepared yet. After each landed load
  the world's catalogue is published again, for the loads prepared after it.
- **The lease.** A prepared load is good only while the world's components are
  the ones it was prepared against: a **lease** on the catalogue's generation.
  It is checked when the prepared load is handed to the frame, and again at the
  boundary. A load whose lease failed is given back to the `loading` family with
  its hold — its ids — ahead of every load filed since, past the 64 bound, and the
  turn stops there: the loads after it wait for it.
- **Cancelling.** A prepared load cancelled is dropped at once, its ids given back;
  one not prepared yet is dropped when it arrives. A cancelled load files no
  outcome: `take_outcome` answers `None` for it.
- **Failures stay with their load.** A prepare that fails, or panics, is that
  load's error; the loads after it go on. A load filed before any world was
  published to load into fails with that reason. A load refused because 64
  already wait lands its error at once.
- **Replace.** A `Replace` or a `Game` load empties the world only when it is for
  that world; one prepared for another world reports `NotLoaded` and leaves the
  world as it is.

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

An asset reference looks the same whatever it names: a uuid. So a value
saved in a field declared `Mesh` and read into a field now declared `Sound`
cannot be caught by its shape, only by the declared types, and it is: two asset
kinds that differ, anywhere inside an optional, an array or a struct, are a
clash. The value is dropped, reported `Dropped`, and the field keeps its
default. This is checked when a record is read into a world, when a prefab is
expanded into the record linking it (its cells read as the linking record
declares them), and when values are staged, for every load, snapshots and game
saves included.

## When a script's declaration changes

A script's components are not fixed when the game is built: the author edits
the declaration and saves, and the rows already stored — in the running world,
in the editor's scene — must follow. They do, at the next frame boundary,
without a load:

| The author… | The stored rows |
|---|---|
| declares a new component | it is registered, in its domain; a behavior's state companion and a system's component as `Runtime` and resumable, any other as `Authored` |
| adds, renames or retypes a field | every row is rebuilt by field name (`World::relayout`), each value read into its new field as a load reads it (`PackedLayout::fit`), so a live edit and a load agree on every value: a value its new type holds is kept; `int` to `float` is converted when the float holds the value exactly (every `int` up to 2²⁴), and an `int` past that takes the field's default, as a load drops it; an `int` inside an optional, an array or a struct becomes the float its new type holds there, and a nested `int` a float cannot hold exactly makes the field keep its default, reported inexact; a value the new type refuses — text in a field become an `int?`, a struct of another shape, an asset of another kind — takes the field's default, as does every row of a new field; a field gone is dropped. Every value a row did not keep is reported (see below) |
| changes the component's domain | every row moves to the new domain's pages, values and change ticks intact |
| deletes the declaration | the component stays registered, with its data, **orphaned**: never deleted, listed as a warning in `Problems` (not when its module was unmounted) |
| declares an orphan again | it is **adopted**, and its rows relaid out to the new declaration |

A rebuilt row is stamped as written, so every observer of the component sees it
change. Nothing a relayout loses is silent: its `RelayoutReport` lists, for every
live row, each value it did not keep — the entity, the field, the value it held,
what the row holds now and why (`LossCause`: the field dropped, a value its new
type refuses, an `int` a `float` cannot hold exactly, a number the component
does not keep). A row is live for the component when its entity holds the
component there, in the component's own domain. The registration logs it and
lists it in `Problems` under the declaring module (`ProblemSource::Relayout`),
one warning per field and cause naming the entities and their values: reported
once, at the relayout, and standing until that module's next relayout, and only
while the module declares the component — a module deleted or unmounted takes
its warnings with it, and a component another module declares now leaves none
behind. A relayout that keeps every value, an exact `int` to
`float` among them, reports nothing. A domain change moves values intact and is
logged. A component declared twice, by two modules, is refused the second time
and the first is kept, listed as an error; when the first module stops declaring
it, a module that was refused it inherits it, before it would be orphaned. A
name a Rust component holds can never be declared. A file read later holds the
fields it was saved with, and is read onto the declaration as it is now, as
above. The language side of the same table is in
[Ergon — writing components](../reference/ergon.md).

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
components. The catalogue's generation tells whether it is: a load prepared
against generation *g* is current iff the world's is still *g*. A load that is not
is prepared again from the source it keeps — every load does, wherever it was
prepared — against the world as it is, so the commit that follows is of a current
load; a failure there adds nothing and reports `NotLoaded`. The loads that keep
no source, `apply`'s and a prefab instance's, are prepared and committed under one
exclusive borrow of the world: nothing runs in between to move *g*, so they are
current by construction (were one not, its commit would add nothing rather than
land stale values). A current load's commit brings every reserved id to life and places
every staged row. ∎

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
