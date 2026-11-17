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
(most entities per page) and looks the others up per entity.

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
rather than growing the page list.

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
