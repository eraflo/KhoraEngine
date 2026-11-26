# Assets and the VFS

<p class="kp-lede">A library does not find a book by the shelf it sits on today: shelves get rearranged. It finds it by its catalogue number, and the catalogue says where the book is. Khora finds a game's files — meshes, textures, fonts, sounds — the same way: each has a number that never changes, and a catalogue says where its bytes are, whether on the disk while you make the game or packed into one file when you ship it.</p>

This page explains why assets are addressed by a stable identity, how an identity
becomes a decoded asset, and how the same path serves loose files in development
and a packed archive in a shipped game. For the steps to reference and load an
asset, see [How-to: load assets](../how-to/load-assets.md); for how a decoded asset
then reaches the graphics card, [Residency](./residency.md).

---

## Identity, not paths

An asset is addressed by an **`AssetUUID`** — a 128-bit identifier — not by its
path. Paths change when files are renamed and moved; a reference must not break
when they do. So game code and scenes carry the uuid, and the **virtual file
system** (VFS) resolves it to wherever the bytes live.

By default the uuid is derived from the asset's path relative to the project's
`assets/` folder, written with forward slashes on every system
(`AssetUUID::new_v5("textures/wood.png")`, a version-5 UUID). The path is only the
*default* seed: the moment an asset is renamed or moved in the editor, its uuid is
**frozen** in the project's identity registry (`<project>/.khora/asset-registry.ron`),
so every reference survives the move. Development and a shipped game resolve
through the same registry, so a uuid is the same in both. See
[File formats — asset identity registry](../reference/formats.md#asset-identity-registry).

A **part of another asset** — a mesh, material, texture or scene inside a glTF
file — has a uuid of its own, derived from its container's uuid and its name
inside it (`AssetUUID::sub_asset`). A renamed glTF keeps its frozen uuid, and with
it every part's.

## The pipeline

The VFS's table is the **asset index** (`AssetIndex`): for each uuid, its
**metadata** (`AssetMetadata`) — its type (`AssetType`, from its extension), where
its bytes are (`AssetSource`), its direct dependencies and its tags. Its bytes are
in one of three places:

| `AssetSource` | Where |
|---|---|
| `Path` | a loose file, in development |
| `Packed` | a span of `data.pack` in a shipped game: offset, size, uncompressed size, compression (none or LZ4) |
| `Sub` | inside another asset: the container's uuid and the part's name |

From a uuid to a decoded asset is one short chain:

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../images/data/asset-path.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>The index.</strong> The uuid is looked up: its type and where its bytes are. No file is opened.</li>
<li><strong>The I/O.</strong> An <code>AssetIo</code> backend reads the raw bytes: <code>FileLoader</code> from the assets folder, <code>PackLoader</code> from the pack. A part of another asset is read from its container.</li>
<li><strong>The reader.</strong> The bytes are checked before any decoder sees them: a pack entry against the signed manifest's hash, any file against its type's size cap and its decoder's recognition of its first bytes. A refused file is never decoded.</li>
<li><strong>The decoder.</strong> The decoder registered under the type's name turns the bytes into a typed asset, within its limits; a decoder that panics is caught.</li>
<li><strong>The service.</strong> The asset is cached under its uuid and an <code>AssetHandle</code> is handed back.</li>
<li><strong>Two ways in.</strong> <code>load</code> does it all on the calling thread; <code>request</code> files it to the <code>loading</code> family, off the frame.</li>
</ol>
</figure>

The **`AssetService`** (`khora-io/src/asset/asset_service/`) is the front of that
chain. It holds the index, the decoders, the I/O backend and a cache per asset
type. An **`AssetHandle<T>`** is a shared pointer to one decoded copy: cloning it
is cheap, and many entities sharing a mesh share one copy in memory.

- **`load::<A>(&uuid)`** reads and decodes on the calling thread, caches, and
  returns the handle — or the cached one if the asset is already loaded. It
  returns an error when the uuid is not indexed, its bytes cannot be read, or
  they do not decode as `A`.
- **`request::<A>(&uuid)`** returns the cached copy at once if there is one
  (`Requested::Ready`); otherwise it files the decode and answers
  `Requested::Pending` until it lands, or `Requested::Failed` once its last decode
  failed and no asset changed since. The **`loading` family** decodes what was
  filed, away from the frame, and the `asset_intake` DataSystem takes the results
  in at the next frame boundary.

The engine's own systems only request: the asset resolver (below) files what it
lacks and stores it on a later frame, so no frame waits for the disk.

Loading is a **service**, and decoding is a **family**. The service has nothing
to choose: an asset is in its cache or it is not. The decoding work, though, is
done by the `loading` family's lane, which runs under the budget like every other
family (see [Agents and Lanes](./agents-and-lanes.md)), so a burst of decodes
competes for the frame instead of stealing it.

The `loading` family loads more than assets: project indexes, scene loads and
script compiles go through it too. Its two lanes differ in how much one run
takes (the levels are listed in
[Families and levels](./families-and-levels.md#the-families-the-engine-ships)):

- **rush** loads everything filed, in one run, whatever it costs;
- **paced** loads what the time it is lent covers, and always at least one unit
  — one project index, one scene load, one script root, one asset decode: the
  project indexes first, then the scenes, the script roots and the assets, each
  kind in the order it was filed. The rest waits for the next run. Lent no
  budget, it takes everything, as rush does.

The decoder set grows by registration: a new format is a decoder registered
under a type name, not a change to the chain (see
[How-to: add an asset decoder](../how-to/add-an-asset-decoder.md)). The same holds
for the I/O: another source is another `AssetIo` behind the same index and
decoders.

## From reference to copy

An entity holds one thing for each asset it uses: its **reference**, the
component that names it (`MeshRef`, `MaterialRef`). The loaded copy lives in the
**asset store** (`AssetStore`, one `Assets<T>` table per type) and the GPU copy
in its [residency](./residency.md)'s table, both under the reference's uuid.
Nothing else is attached to the entity: remove the reference and the entity
names nothing, and draws nothing.

Every reference has a uuid:

- `MeshRef::Asset(uuid)` and `MaterialRef::Asset(uuid)` name the asset's own;
- a procedural mesh (`MeshRef::Procedural`) has one derived from its kind and
  its parameters, and an inline material (`MaterialRef::Inline`) one derived
  from its fields. Two equal primitives, or two equal materials, share one
  copy. These uuids are never saved: they are computed again on load and on
  every edit;
- a mesh built in code (`MeshRef::Made(uuid)`) names the store's entry it was
  put in. No file holds it, so a scene leaves it out and reports it not saved.

**The resolver.** The **`asset_resolver`** DataSystem, at `PreExtract`, reads
only the references written since its last run (it observes `MeshRef` and
`MaterialRef`, see [Change detection](./change-detection.md)): a frame where no
reference changed costs it no row. For each one:

- a procedural mesh is built, and an inline material copied, into the store
  under its uuid, unless the store holds that copy already;
- an asset is requested from the `AssetService`. One cached is stored now; one
  being decoded lands in the store at a frame boundary, through the
  `asset_intake`, with no reference read again;
- a mesh made in code needs nothing; one the store does not hold draws nothing,
  and is logged.

So changing a reference — another mesh, a material's colour edited in the
inspector — changes its uuid, and its new copy lands in the store under the new
uuid, where its residency makes the GPU copy. A copy the store already holds is
not stored again: two entities naming one asset share one copy, on the CPU and
on the GPU.

**What waits.** The resolver keeps the assets it asked for and has not seen
land, by uuid and type, and asks for them again each frame until they do: one
whose decode was turned down (a file changed under it) is filed again, one
still decoding is not. A **failed decode** is logged once, as a warning, and is
not filed again until a file of the project changes; then it is, with the
reference unchanged. There is no fallback asset. An asset whose copy left the
store because its file was deleted waits the same way, and comes back with its
file.

**Textures follow their material.** The resolver reads what arrives in the
material store: a material that lands or is replaced — stored by the resolver,
decoded, or reloaded — has the textures it names that the store lacks asked
for, and followed the same way until they land.

A UI text's font is resolved by the same system onto the entity, as a
`HandleComponent<Font>` the text systems read; a UI image's texture goes to the
store.

## Dev and packed are the same path

In development, assets are loose files under the project's `assets/` folder. The
index is built by scanning that folder, off the frame: a game started from loose
files starts with an empty service, and the indexed one replaces it at a frame
boundary once the scan is done.

The editor's *Build Game* concatenates every asset into **`data.pack`**, writes
the index as **`index.bin`** with each source rewritten from a path to a span of
the pack, and seals both with a signed **`manifest.bin`** of their hashes. A
shipped game opens the pack only if the manifest's signature verifies with the
key stamped into its binary, and checks every entry it reads against the
manifest's hash (see [Trust — signed packs](./trust.md#signed-packs)). A stamped
binary refuses loose assets beside it.

Above the I/O, nothing knows which mode is in use: the same index type, the same
reader, the same decoders, the same handles. Because the uuids come from the
same paths and the same registry, a handle made in development names the same
asset in the shipped game.

## Textures: a layout, not a colour

The same picture can be two things: a photo of wood, whose values are colours
for the eye, or a normal map, whose values are directions. The first is stored
in **sRGB** — a colour space that spends more of its values on dark shades —
and must be converted to linear values before lighting; the second is already
linear, and converting it would bend every normal.

So the texture decoder says only how the pixels are laid out, never what they
mean:

- an 8-bit image (PNG, JPEG, …) decodes to `Rgba8Unorm`;
- a floating-point image (Radiance `.hdr`, OpenEXR) keeps its high dynamic range
  and decodes to `Rgba16Float` — half floats, which every graphics backend can
  filter, where 32-bit float filtering is optional.

What the values mean is decided by the slot that reads the texture
(`TextureColorSpace`, when the material is uploaded): base colour, emissive and
specular colour are sRGB; metallic-roughness, normal, occlusion and specular
strength are linear. Only an 8-bit layout has both variants, so only it switches
(`TextureFormat::with_color_space`); a float texture is linear whatever the slot
asks. One PNG can be an sRGB albedo in one material and a linear mask in
another.

## glTF files

A glTF file (`.gltf`, `.glb`) is a whole small scene: nodes placed in a tree,
meshes, materials, images. The engine reads it two ways.

**As one mesh.** Its own uuid decodes to one `Mesh`: every primitive its default
scene shows (else its first scene; a file with no scene shows each mesh once),
placed by its nodes, merged into one vertex and index list. That is what
`MeshRef::Asset` of the file draws.

**As its parts.** Each piece is also an asset of its own, a **sub-asset**, named
by a **label** inside the file and given the uuid
`AssetUUID::sub_asset(file's uuid, label)`. The parts are listed from the file's
JSON alone, when the index is built, without reading a buffer or an image:

| Label | Asset |
|---|---|
| `scene` | the default scene (else the first) — a prefab |
| `scene{i}` | scene *i* — a prefab |
| `mesh{m}/prim{p}` | primitive *p* of mesh *m*, in the mesh's own space |
| `material{i}` | material *i*, as a `StandardMaterial` |
| `material_default` | glTF's default material, for primitives without one — listed only when one uses it |
| `texture{i}` | the image of texture *i* |

Labels follow the file's order, like the glTF indices they come from. A file is
opened once and kept, shared by every part extracted from it, until it changes.

**A scene is a prefab.** Placed in a level, a glTF scene is a
[prefab instance](./prefabs.md): one entity per node, each showing its
primitives' mesh and material parts, the level keeping the link and the
designer's overrides. An override is kept against the member it was made on, so
a member's identity must survive a re-export. It comes from the node's **path of
names** from the scene's root — `Body[0]/Wheel[1]` — not from its index: an
exporter reorders nodes freely, while renaming one is the author's act.

- A segment is the node's name, with `\`, `/`, `[`, `]` and `#` escaped, and its
  ordinal among the earlier siblings of that name: `Wheel[0]`, then `Wheel[1]`,
  so adding a second `Wheel` leaves the first one's key alone.
- An unnamed node is `#i`, its place among its siblings.
- A mesh's primitives past the first are child entities, `#prim{p}`; no escaped
  name starts with `#`.
- A member's key is derived from its parent's key and its own segment, so a
  deep tree costs its size, not its size times its depth.

An override whose node was renamed no longer finds its member: the load reports
it as `RemovedFromScene`, rather than handing it to another node.

**The files beside it.** A glTF's buffers and images are embedded (in a `.glb`'s
binary chunk), written into a base64 `data:` URI, or stored in files beside it.
A file is named by a URI relative to the glTF's own folder, so an export
(`ship.gltf` beside `ship.bin`) works wherever it sits under `assets/`. A URI is
percent-decoded first; one that is absolute, names a scheme other than `data:`,
or climbs past the assets root with `..` is refused. The file is then read
through the [guard](./trust.md#the-guard), at the project's trust; in a shipped
game it is read from the pack.

**What a glTF may ask for.** A glTF is untrusted input: a few bytes of JSON can
describe gigabytes. Before anything is imported, every accessor and image is
checked — its type fits the attribute naming it, its bytes lie inside its buffer,
its sparse indices increase — and the node tree must be disjoint trees: a node
reached twice fails the decode. One decoded mesh holds at most 2²⁴ vertices in
a trusted project (2²² restricted), a mesh placed under many nodes counting
once per placement; an accessor with no buffer behind it describes at most a
sixteenth of that.

## Dependency tracking

An asset's metadata records its **direct dependencies** — the uuids of the other
assets it references — so a loader can fetch them without decoding the asset
first. They are extracted while the index is built, by one function keyed on the
asset's type name (`khora-io/src/asset/dependencies.rs`).

Two types are extracted:

- a **material** (`.kmat`) records the textures its slots name — base colour,
  metallic-roughness, normal, emissive, occlusion, specular, specular colour —
  copied from the uuids the material stores;
- a **script** (`.erg`) records the modules it `import`s. An import is written
  from the project's script folder, whatever folder the importing file sits in,
  so `import "combat/damage.erg"` means `scripts/combat/damage.erg` everywhere.
  The import is read in normal form, as the compiler reads it
  (`./combat/damage.erg` is `combat/damage.erg`); one the compiler refuses, such
  as a path leaving the script folder, is no dependency, nor is a module the
  engine serves from memory (`engine/components.erg`, `engine/events.erg`).

A dependency named by a path, like an import, is given the uuid the index gives
the file at that path: the builder hands the extractor the resolution it uses for
its own entries, through the identity registry. A script importing a renamed
module therefore depends on the module's frozen uuid, and one importing a new
file at a renamed asset's old path depends on that file's derived uuid — the
same in development and in a pack. A material's textures need no resolving: the
material stores their uuids.

The list is deduplicated and sorted, so the index is the same bytes from run to
run. The other types return an empty list — scenes, prefabs and meshes among
them. A scene holding [prefab instances](./prefabs.md) keeps each as a link to
the prefab's uuid, and that link is **not** recorded as a dependency: the
serialization service reads each prefab when it expands the scene, but the index
cannot tell which scenes use a prefab. Adding an extractor is one match arm. To
keep the scan fast, bytes are read only for a type that has an extractor:
textures and sounds are never opened.

## When an asset changes

Save a texture in your paint program while the game runs in the editor, and the
wall wearing it changes on screen a moment later. Nothing else stops: the wall
keeps its old picture until the new one is ready, and keeps it for good if the
new file turns out to be broken. This is **hot reload**.

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../images/data/hot-reload.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>A file is saved.</strong> The watcher hears the change and passes it on, if the folder's budget of changes for this second is not spent.</li>
<li><strong>Invalidated.</strong> The asset service drops its cached copy of the file, of every part it served, and of every container that read the file beside its own, and announces each on its change stream.</li>
<li><strong>Asked for again.</strong> At <code>PreExtract</code>, <code>asset_reload</code> takes the announced changes and requests a fresh copy of each one the asset store holds. The decode runs off the frame; meanwhile the last good copy keeps serving.</li>
<li><strong>The new copy lands.</strong> It replaces the old one in the store under the same uuid, which every entity naming it reads. The residency reads that arrival and makes the GPU copy again; the one made from the old copy is retired.</li>
<li><strong>Or it does not decode.</strong> The file was caught half-written: the last good copy stays, and is kept until the file changes again.</li>
</ol>
</figure>

**Hearing the change.** While a project is open in the editor, its `assets/`
folder is watched (`AssetWatcher`). One save often fires several identical
events in a row; they collapse into one. A modified file is invalidated; a file
created or removed — a rename is both — makes the project indexed again, and so
does a change to a file that holds parts (a glTF may have gained or lost some).

A folder lets through at most **512 change events a second**. A flood past that
— a branch switch, a project-wide reformat — drops the events it cannot carry,
and once it has been quiet for a second, one **rescan** stands for all of them:
every indexed file is invalidated and the folder is indexed again. The engine's
script and shader pumps read the same watch, and recompile or reapply
everything of a rescanned folder (below).

**Invalidating.** `AssetService::invalidate` drops the cached copy and
**announces** the change; handles already out keep the old value. Invalidation
spreads:

- to every part the file served (a glTF's meshes, materials, textures), even
  one a new index no longer lists, since the engine may still hold its copy;
- to every container that read the file beside its own — a glTF and its `.bin`
  — and from there to that container's parts, and so on.

A container names the files it read by path, and a path is matched by the uuid
it resolves to, so a change to a renamed asset never reaches a container reading
the new file at its old path. A file the editor saw change, or released from the
[quarantine](./trust.md), resolves through the identity registry
(`AssetService::invalidate_through`): one made since the last reindex reaches
the containers reading it. An asset invalidated by uuid alone resolves through
the index (`AssetIndex::uuid_at`). A decode already running for a changed file
is of its old bytes: its result is never installed.

Announcements wait on a **change stream** of 4 096 entries. One past that pushes
out the oldest, and the announcements lost cannot be told apart: the next reload
then treats every asset the service ever served as changed, with a warning.
Reloading too much costs time; missing a change would leave a stale copy on
screen.

**Reloading.** The **`asset_reload`** DataSystem, early in `PreExtract`, takes
the announced changes and requests a fresh copy of every changed texture,
material and mesh the asset store holds, and every changed font a text holds. It
visits no entity: one decode per asset, however many entities name it:

- until the new copy lands, the **last good copy** keeps serving — and a
  reference resolved meanwhile is served it too, so a new entity shows what the
  others show; an asset still decoding is looked at again on the next frame;
- once it lands, it replaces the old one in the store under the same uuid. The
  reload touches no GPU copy: the store records the
  replacement as an arrival, and the residencies make the GPU copy again, and
  every material naming a changed texture, and retire the old ones (see
  [Residency](./residency.md#how-a-residency-is-fed)); a changed font lays its
  texts out anew;
- a file that no longer decodes — caught mid-write — keeps the last good copy,
  in the service too, until it changes again;
- an asset gone from the index, or a part its container no longer holds, is
  dropped from the store; the references naming it stay, and draw nothing
  until it is back;
- a changed asset the store does not hold has no copy to replace: the resolver
  decodes it afresh when a reference names it again;
- a stored material naming a changed texture the store lacks has that texture
  asked for again.

When a project is replaced by another, every asset the old one served is
announced, and none of its copies is ever kept as a last good copy of the new
project's asset.

**Scripts and shaders** are not reloaded as assets. A changed `.erg` under
`assets/scripts/` is compiled again off the frame, together with **every module
that imports it**, directly or through others: editing a module changes what its
importers compile to. The importers are found by reading the files' `import`
lines, never from the asset index — an `.erg` edited in place is not indexed
again, so the index would miss an `import` just added. A save that does not
compile replaces nothing. A changed `.wgsl` that names a module the engine's
shaders compose is a [shader overlay](./trust.md#shader-overlays).

## What stays in memory

The service keeps every asset it decoded until that asset changes: dropping the
last handle does not unload it, and loading it again is a cache hit. What is
freed while the game runs is the **device** side: the GPU meshes and materials no
live entity references are released by the eviction service, a few each frame
(see [Residency — eviction](./residency.md#eviction)).

## Under the hood

**Identity.** Write *key(p)* for the forward-slash form of a path *p* relative to
`assets/`, *R* for the identity registry (a map from keys to frozen uuids) and
*v5(N, b)* for the version-5 UUID of the bytes *b* in the namespace *N*. An
indexed file's uuid is

> *uuid(p)* = *R(key(p))* if the registry holds *key(p)*, else the first of
> *v5(N_asset, key(p))*, *v5(N_asset, key(p) ‖ NUL ‖ 1)*, *v5(N_asset, key(p) ‖ NUL ‖ 2)*, …
> that *R* gives no other key (the default, or a derived uuid),

and a part labelled *ℓ* of a container *c* has *v5(N_sub, bytes(c) ‖ ℓ)*. The two
namespaces are distinct constants, so a part's uuid never equals a path's.
*uuid* depends only on *key(p)* and *R*, never on the scan's order or the system
it runs on; the index sorts its entries by key, so the index of one folder and
one registry is the same on every run. A dependency on the path *q* is
*uuid(q)*, by the same function: for every indexed file *q*, an asset that names
*q* depends on exactly the uuid *q* is indexed under.

**The request contract.** For one uuid and one type, `request` files at most one
decode while one is pending. Between two asset changes it answers `Failed` for a
uuid whose last decode failed, so a system resolving every frame does not retry a
broken file every frame; any change clears it.

**The watcher's budget.** Each watched folder holds a token bucket of capacity
*b* = 512, refilled at *b* tokens a second; an event passes when it takes a
token. Over any window of *s* seconds, at most *b* (1 + *s*) events pass. A
dropped event sets a mark; the first poll at least one second after the last
drop sends one `Rescan` and clears the mark. So a flood of any size costs at
most *b* (1 + *s*) events and one rescan, and no change is lost: a dropped event
is always followed by a rescan, which invalidates every indexed file.

**No change missed.** Write *C* for the change stream (capacity 4 096, oldest
dropped first) and *L* for its lost flag. Every invalidation sends its uuid on
*C*, and sets *L* when the send pushed one out. `take_changes` returns the
drained stream, and, when *L* was set, every uuid the service ever served. So
the set returned always contains every asset invalidated since the last call
that the engine may hold a copy of.

**What a check guarantees.** A decoder receives only bytes that passed the
reader: within the size cap of their type at the project's trust, recognised by
the decoder's `sniff`, and — from a pack — hashing to the manifest's entry. The
formal statement of the guard is in [Trust — under the hood](./trust.md#under-the-hood).

## Next steps

- [How-to: load assets](../how-to/load-assets.md) — reference an asset from a
  component, and load one from code.
- [How-to: add an asset decoder](../how-to/add-an-asset-decoder.md) — register a
  decoder for a new format.
- [Residency](./residency.md) — how a decoded asset reaches the GPU, and leaves it.
- [Trust](./trust.md) — the guard every file is read through, and signed packs.
- [Serialization](./serialization.md) — scenes, which reference assets by uuid.
- [Glossary](../reference/glossary.md) — AssetUUID, VFS, decoder, service.
