# Data and the ECS (CRPECS)

<p class="kp-lede">Picture a game's world as a filing room. Every thing in the game — a lamp, a player, a sound — is a number on a list, and what it is made of is written on cards: where it stands, what it looks like, how heavy it is. Cards of the same kind are filed together in drawers, so a clerk who needs every position in the room opens one drawer and reads it top to bottom. Khora's data layer is that filing room, built so that adding a card, finding a drawer and copying a whole drawer to disk are all cheap.</p>

This page explains how the engine stores its data, why it is shaped that way, and
what the shape gives the rest of the engine. For the steps to define a component,
see [How-to: add a component](../how-to/add-a-component.md); for the type-level
facts, the rustdoc on `khora_data::ecs`.

---

## Why a custom ECS

An **ECS** (entity–component–system) stores a game as **entities** — identifiers
with no data of their own — and **components** — plain data attached to them —
and runs **systems** over every entity that holds a given set of components.

Khora's is **CRPECS** — *Chunked Relational Page ECS*. Its storage is **chunked**
into pages, the link between an entity and its data is **relational** (an entity
is an identifier, never a pointer), and the **page** is the unit of everything:
iteration, compaction, change tracking, saving.

The engine needs three things at once from its storage:

- adding or removing a component must cost a bounded amount of work, not a pass
  over the world;
- a system must read its components in long contiguous runs, the way the
  processor's cache and its vector instructions want them;
- the data's *layout* must be able to change under the hood without changing what
  the data means — the promise of [AGDF](./agdf.md).

A sparse-set ECS makes the first cheap and the second costly; a pure archetype
store does the reverse. CRPECS groups an entity's components by **semantic domain**
(below), so a structural change moves one row of one domain, while each page stays
dense.

## The storage model

A `World` holds three things: an **entity store**, a **component registry** and
the **pages**.

<div class="kp-figure-frame">

{{#include ../images/data/world-pages.svg}}

</div>

**Entities** are an index and a **generation**. When an entity is despawned and
its slot reused, the generation moves on, so an old `EntityId` fails its lookups
instead of naming whatever lives in the slot now. For each domain the entity has
components in, the store records the page and the row holding them.

An `EntityId` means nothing once its world is gone. What a save knows an entity by
is its **`PersistentId`** (`khora_core::ecs::PersistentId`), which the `World`
keeps beside each live entity — not as a component, so no query sees it. Its 64
bits hold two namespaces: the *authored* one (random 63-bit ids drawn when the
editor creates an entity, so two people adding entities on two branches do not
collide, and ids derived with `within` or `named`), and the *created* one, marked
by the top bit, numbered by the world for entities the game spawns, and given
only the first time something asks. `World::persistent_id` and
`World::entity_with_id` translate between the two. How a scene is written down by
these ids is the subject of [Serialization](./serialization.md).

**Reserved ids.** An entity's id can be taken before the entity exists, by code
that does not hold the `World`: a script spawning an entity names it at once, so
its later commands in the same turn can use it; a scene load prepared on another
thread holds one id per entity it will add. A **reserved** id is not alive — no
query, `get` or spawn sees it — and is never the id of a live entity or of
another reservation. It becomes an entity only when its owner spawns it there
(`World::spawn_at`), or goes back unused:

- a script's reservations (`World::reserver()`) are settled each frame: the ids
  its commands spawned live, the others are released at the frame's end, keeping
  a few (at least 16) ready for the next frame;
- a load's reservations are held in a `ReservationHold`
  (`World::hold_reservations()`), across frames, until the load commits;
  dropping the hold gives back every id not committed.

An id given back that never lived is handed out again as it is; a slot freed by
a despawn comes back with its generation moved on, so an old id never names its
new owner.

One reserved id is special: the **nowhere** entity, reserved once when the world
is made, never alive and never spent (`World::nowhere_entity()`). A saved
reference to an entity a load does not hold is bound to it, so it can never come
to name an entity spawned later (see
[Serialization](./serialization.md#pages-persistent-ids-provenance)).

**Components** are plain data tagged with `#[derive(Component)]`. The derive
generates the component's serialisable mirror and its `From` conversions, and
registers the type through `inventory`, so `World::new` finds it with no list to
maintain. It generates a mirror rather than relying on reflection: two structs kept
by hand drift apart, and reflection would allocate on the hot path.

The same derive carries what saving and checking need to know, as attributes:

- `domain = …` — the component's semantic domain. Without it the type is not
  registered and `World::spawn` refuses it.
- `provenance = …` — who writes it (below).
- `resumable` — runtime state a **game save** keeps anyway, because play cannot
  resume without it. No Rust component has it; a behavior's state companion,
  declared while the engine runs, is resumable.
- `formerly = "OldName"` — on the type or on a field: a save written under the
  older name still loads, read as today's name and reported as renamed.
- `check = Type::check` — a function naming the values the engine cannot use
  (a camera's near plane at 0). A check never rewrites nor refuses a value: a
  load reports it, the running world reports it to `Problems`, the inspector
  shows it under the field.
- `layout = "soa"` — a field-split column (see [AGDF](./agdf.md)).

What these change on disk, and why a removed type has to be declared retired, is
covered in [Serialization](./serialization.md) and
[How-to: add a component](../how-to/add-a-component.md).

### Provenance

A component's **provenance** says who writes it, and so what the engine may do
with it on your behalf:

| Provenance | Example | Offered in *Add Component* | Copied when an entity is duplicated | Saved |
|---|---|---|---|---|
| `Authored` (default) | `Transform`, `Collider` | yes | yes | in scenes |
| `ToolAuthored` | `Parent`, set by dragging in the scene tree | no | yes | in scenes |
| `Derived` | `GlobalTransform`, recomputed from `Transform` and `Parent` | no | no | never |
| `Runtime` | `PhysicsDebugData` | no | no | in game saves, if `resumable` |

Provenance is a different question from the [domain](#semantic-domains), which
says who *reads* a component. `Transform` and `GlobalTransform` share a domain —
both spatial — but a person writes one and the engine recomputes the other each
frame. `Collider` and `PhysicsDebugData` share the physics domain, but one is an
input and the other debug output. Without provenance, *Add Component*, duplicate
and scene save would each keep their own list of what to leave out, and none of
them would know a component defined outside the engine.

A tool-authored component is saved and copied, but never offered by hand: an
empty `Parent` added from a menu means nothing. A derived one is never copied,
because a copy would be stale until recomputed.

### Registration

A component is stored only if its type is registered — by the derive, or in
`World::new` for the few written by hand (generic ones such as
`HandleComponent<T>`). `World::spawn` panics on one that is not, and on a bundle
naming a component twice; `World::try_spawn` returns the error instead
(`SpawnError::ComponentNotRegistered`, `SpawnError::ComponentNamedTwice`), before
anything is allocated. An unregistered component, stored, would sit where no query
or `get` could reach it. It is a programming error, the same every time the code
runs, so it is refused at once rather than left to be found later.

A registration also publishes the component's **shape** (`ComponentShape`): its
fields by name, each with its Rust type as written, `#[component(skip)]` fields
left out; an empty list is a marker component. The few hand-written
registrations whose value is one of several forms (an asset reference,
`AssetRef<Mesh>`, `AssetRef<Box<dyn Material>>`) say `Opaque`. Scripts read the engine's components through it: the
`engine/components.erg` module a script imports is generated from the shapes, so
a misspelt field is refused when the script compiles.

### Pages

A **page** (`ComponentPage`) holds the rows of the entities that share one
**signature** — one exact set of component types. It keeps one column per
component of the signature, the entity of each row, and a tick per row and column
(see [Knowing what changed](./change-detection.md)). Rows are dense: a page has no
holes to skip.

An entity's components are grouped **by domain**: each group is one row, in a page
whose signature is that group. A spawned bundle that spans several domains starts
in one page shared by them. Adding or removing a component moves only the row of
that component's domain:

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../images/data/migration.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>One row per domain.</strong> e7's physics components sit in one row of page P, whose signature is <code>RigidBody</code> alone.</li>
<li><strong>A <code>Collider</code> is added.</strong> e7's physics row is copied into page Q, the new value beside it, and its location points there. The old row stays behind as an orphan: no location names it, so every query skips it.</li>
<li><strong>Maintenance reclaims it.</strong> At the end of a frame, the orphan is removed and the page's last row takes its place. The order changed, so the domain's epoch rises.</li>
</ol>
</figure>

`World::remove_component_domain::<C>` drops every component of `C`'s domain from
an entity in O(1): only its location changes, and the row waits for compaction.

**A page describes itself.** Its columns are keyed by the component's own
**`ComponentKey`** — a Rust component's `TypeId` — so reading a column needs the
page and the key, never the `World` that filled it, and a component has the same
key in every world. A column is found in constant time whatever the page's width:
the key is already a hash, and the page's map keeps it as one (`KeyHasher`)
instead of hashing it again on every row a query visits.

**The vtable.** The registry holds one `ComponentVTable` per component: its key,
its one short name (what scenes, the editor and scripts call it), its domain, its
layout, its provenance, whether it is resumable, and the operations storage needs
on its columns — create one, copy a row into another page. Storage calls those
operations and never learns a column's concrete type; that is what lets a
field-split column and a plain `Vec<T>` sit side by side, and what lets a
component exist with no Rust type at all. The same table reads and writes a
component's fields one at a time, by slot — what a script's
`e.Set(Health { current: 5 })` becomes at the frame boundary: the one field
written in place, nothing cloned — and takes a typed copy of a column that can be
read row by row without the `World`. What a field is to a script is decided once,
by its Rust type (`ScriptField`): a type that would not survive the trip, such as
`u64` or `f64`, is out of a script's reach.

**Components declared while the engine runs.** A script can declare a component
that no Rust type stands for. It is registered with its fields and its domain,
keyed by `ComponentKey::named(name)` — a stable hash of its name, the same in
every world and every run — and stored in its domain's pages beside the Rust
components, migrating with its entity as they do. Its rows pack each fixed-size
field inline and keep strings, arrays and structs beside them, deep-copied with
the row. Changing its fields (a script reload) rebuilds every row by field name: a
field keeps its value while its name and kind are unchanged, and takes its default
otherwise. A name belongs to one component only, Rust or declared, and two Rust
types with one short name are refused when the world is built.

## Semantic domains

Every component declares one **`SemanticDomain`** — `Spatial`, `Render`, `Audio`,
`Physics`, `Ui`, `Script`. The domain decides which pages hold it and which
consumers care when it changes.

The `World` keeps, per domain, the set of entities that have components in it.
A query's plan takes the domains of its terms: a query within one domain walks
that domain's pages only, so a render extraction never touches UI pages. A query
spanning several domains is **transversal**: it is driven by the densest of them
(most entities per page) and looks the others up per entity. The driver walks its
domain's pages; for each row, it checks that the entity is in every other domain
the query names and that the row is still the entity's live row in the driver's
domain, then reads each term from the entity's own location. A query's plan is
made once per set of component types and kept until a component is registered or
changes domain: the driver is not chosen again as the world fills.

Domains are also the coarsest unit of *change tracking*. The `World` keeps a
**change epoch** per domain: the tick of the latest write to one of its values, or
of the latest move of a row of one of its pages — a spawn, a despawn, a component
added or removed, a compaction that reorders rows. Handing out writable access
(`get_mut`, a mutable query) moves nothing: only a write does. The epoch is read in
O(1).

The contract is one-sided. Equal epochs across two reads *guarantee* the domain's
data — and its iteration order — is unchanged; a different value means only
"possibly changed". A raise too many is harmless, while a *missed* one would leave
a consumer stale. Epochs are read with `World::domain_epoch(domain)` and paired with
`World::instance_id()`, so epochs of two different `World`s never compare equal.

Finer answers — *which* rows of a component changed since a given moment — come
from the ticks every page keeps beside its columns, and the `Changed<T>` query
term: see [Knowing what changed](./change-detection.md).

The first consumer of the epochs is the [Flows](./agdf.md#flow-view-caching--per-domain-change-epochs):
a Flow that can name all of its inputs folds their domains' epochs into a cache
key, and on an unchanged frame republishes its previous View instead of projecting
it again.

## The hierarchy

Entities form a tree: a wheel under a car, a sword under a hand. The tree is
stored twice, once on each side of every link, so both questions are cheap —
"who is my parent?" and "who are my children?":

| Component | On | Holds | Provenance |
|---|---|---|---|
| `Parent` | the child | its parent | `ToolAuthored`: saved |
| `Children` | the parent | its children, in order | `Derived`: never saved, built from the `Parent`s |

Two copies can disagree, so one function writes both: `World::set_parent(child,
parent)`. It takes the child out of its former parent's `Children`, then sets its
`Parent` and appends it to the new parent's `Children` (`None` makes it a root).
It refuses — returns `false`, touching nothing — a missing entity, an entity as
its own parent, and a parent that is already beneath the child: the tree never
holds a cycle. Writing `Parent` by name goes through it, and `Children` cannot be
written by name at all; a script asking for a cycle is refused with an error
(`WouldCycle`). A load reads the tree from the saved `Parent`s, refuses a file
whose `Parent`s form a cycle, and builds each `Children` in the saved order.

**Removing a branch.** `World::despawn` removes one entity and touches no other:
its children keep a `Parent` naming it, and its parent's `Children` still lists
it — dead ids, which fail every lookup. To remove an entity with everything
beneath it, call `World::despawn_subtree(root)`: it collects the whole branch
first, takes the root out of its parent's `Children`, then despawns each entity,
and returns how many went. A script's `Despawn` removes the subtree.

**Where things are in the world.** `Transform` is where an entity stands
relative to its parent; `GlobalTransform`, where it stands in the world, is
derived from the chain of `Transform`s each frame by `transform_propagation`.
Being derived, it is never saved, so a loaded entity arrives without one. The
`ensure_global_transform` DataSystem gives one to every entity holding a
`Transform` and lacking it, early in `PostSimulation`, seeded from the entity's
own `Transform` rather than the origin — so an entity does not slide in from the
origin on its first frame.

## Queries and SoA versus AoSoA

Queries are typed and borrow-checked at compile time. A query asks for a tuple of
terms, and its plan picks every page whose signature holds the components it needs:

```rust
for (transform, mut global, _) in
    world.query_mut::<(&Transform, &mut GlobalTransform, Without<Parent>)>()
{
    global.set_if_neq(GlobalTransform(transform.to_mat4().into()));
}
```

Reading and writing are two doors:

- `world.query` takes `&World` and accepts only read-only terms: `&T`,
  `Option<&T>`, `EntityId`, `Without<T>`, `Changed<T>`, `Soa<T>`. Asking it for a
  `&mut T` is a compile error.
- `world.query_mut` takes `&mut World`. A `&mut T` term yields a `Mut<T>` guard,
  which reads like a `&mut T` and stamps the row as changed on its first write;
  `set_if_neq` writes only a value that differs, so a still scene raises nothing.
  It panics on a query naming one component twice, which would hand out two
  references to one value.

A query skips the orphan rows a migration left behind, so it never yields an
entity twice. Spawning is held to the same rule as adding: `world.spawn` panics on
an unregistered component, `world.try_spawn` returns the refusal.

**Writing a column at once.** A script that sets the same fields on many
entities in one turn hands the frame one batch, not one command per entity.
`World::write_column(component, slots, rows)` writes the given field slots of
each listed entity: it sorts the rows by page and row, so the write sweeps
memory in order, and writes each run of one page in one call. Of two rows for
one entity, the later wins. Each row written is stamped, as a guard's write is;
nothing moves, so no page's order changes. It returns what it did
(`ColumnWriteReport`): the rows `written`, the `missing` ones — an entity gone,
or without the component — and the `refused` ones, whose width or a field's
value did not fit; a refused row is skipped whole.

Underneath, a default column is a `Vec<T>`: **SoA** (structure of arrays) *across
entities*, but **AoS** (array of structures) *within* a component — whole structs
back to back. That is the right default and what every mainstream archetype ECS
does. It leaves the *field* level on the table: a loop touching one field of a fat
component strides over all the others, wasting cache lines and defeating SIMD
(one instruction over several values). A compute-heavy, all-`f32` component can
opt into a **field-split** column, where each field is its own contiguous stream,
read through the `Soa<T>` term by value. The trade-offs, the measured wins and why
the layout is chosen per component are covered in [AGDF](./agdf.md).

## Maintenance is a service, not an agent

Migrations leave orphan rows behind. Reclaiming them is **`EcsMaintenance`**'s
job: a runtime resource the `ecs_maintenance` DataSystem ticks in the
`Maintenance` phase, at the end of the frame, after the families ran. Each tick it
compacts up to 10 dirty pages — pages a migration left an orphan in — and the rest
wait for the next frame, queries skipping their orphans meanwhile. The same system
frees the row ticks no observer needs any more. A drained page's slot is reused
rather than growing the page list; a page's slot is never removed, since entity
locations name pages by slot. A reused slot carries its predecessor's
[order version](./change-detection.md#stickers-on-the-rows) on, one step
further, so a reader remembering "slot 4, order 7" never finds that pair again
on a page holding other entities.

Maintenance is deliberately *not* an [agent](./agents-and-lanes.md). An agent
exists to choose between levels of work under a budget; maintenance has nothing to
choose, it does the same bounded work every frame. So it is a direct data-layer
operation, registered as a `DataSystem` with the role `Upkeep`. Knowing where that
line falls — subsystems that choose are families of lanes, fixed operations are
services and systems — is the most useful distinction for reading the data layer.
The same line puts the GPU's half of the cleanup, freeing the meshes and materials
no entity uses, in a service: see [Residency](./residency.md#eviction).

## Under the hood

**Notation.** An entity is a pair *(i, g)*: an index and a generation. The store
holds, per index, the current generation *G(i)* and, per domain *d*, a location
*L(i, d) = (p, r)*: page *p*, row *r*.

**Stale handles.** A lookup of *(i, g)* succeeds only if *g = G(i)*. Despawning
*i* increments *G(i)* before the slot is reused, so every handle taken before the
despawn fails its lookups for as long as the generation does not wrap.

**Orphans.** A row *r* of page *p* is **live** when some live entity has
*L(i, d) = (p, r)* for some domain *d*, and an orphan otherwise. Every query yields
live rows only, so each matching entity appears exactly once. Compaction removes
exactly the orphans of a dirty page, by swap-remove, and raises the epoch of every
domain of that page's signature when it removed one.

- Liveness is checked in every domain, with the generation: a row of a page
  shared by several domains that is still live for one of them is kept, and a
  row whose owner died or whose index was recycled is an orphan.
- A swap-remove moves the page's last row into the hole. Compaction then
  repoints every location of the moved row's entity that named the last row —
  matched by row, not by entity, since an entity that left a page and came back
  has both an orphan and a live row there under one id.
- A moved row whose owner is dead has no location to repoint; it is an orphan
  still, removed by a later step.
- A despawn removes a row shared by several domains once, not once per domain.

**Transversal plans.** A query over domains *D₁ … Dₖ* (*k* ≥ 2) is driven by the
domain of greatest density *nₑ / nₚ* (entities over pages), compared by
cross-multiplying. A page holding several domains, made by a spawn spanning
them, is counted in the page total of one domain only, its first. The
iteration applies no page filter for the other domains: a driver page may be a
shared page whose other columns are stale, so each row is checked against the
entity's domain set and its live row, then read through its live locations.

**Checked under Miri.** Two suites pin what the type system cannot: a write
through `query_mut` never goes through a pointer derived from a read-only one
(`khora-data/src/ecs/tests/query_mut_writes.rs`), and items of one query held at once never
alias (`query_mut_aliasing.rs` beside it). They run under Miri, the Rust
interpreter that detects undefined behaviour, by hand:
`cargo +nightly miri test -p khora-data --lib query_mut`. `cargo gate` does not
run them.

**Costs.** Adding a component to an entity copies one row of one domain: O(*c*),
*c* the number of components of that domain the entity holds, independent of the
world's size. Removing a whole domain is O(1). A column lookup is one hash-map
probe on a key that is already a hash. Maintenance does at most 10 page
compactions per frame, so a burst of *N* dirtied pages is cleared within
⌈*N* / 10⌉ frames.

**The epoch contract.** For a domain *d*, write *E_d(t)* for its epoch read at
time *t*. If *E_d(t₁) = E_d(t₂)* then no value of *d* was written and no row of a
page of *d* moved between *t₁* and *t₂*. The converse does not hold. The proof, and
the finer per-row ticks, are in
[Under the hood: change detection](./change-detection-formal.md).

## Next steps

- [How-to: add a component](../how-to/add-a-component.md) — define, tag with a
  domain, and register a component.
- [Knowing what changed](./change-detection.md) — per-row ticks and `Changed<T>`.
- [AGDF](./agdf.md) — the field-split substrate and Flow view caching on top of
  this storage.
- [Agents and Lanes](./agents-and-lanes.md) — what consumes the Views the data
  layer projects.
- [Glossary](../reference/glossary.md) — CRPECS, SemanticDomain, change epoch,
  page.
