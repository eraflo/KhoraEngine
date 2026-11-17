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

The engine's own systems only request. The asset resolver — the `DataSystem` that
turns what entities name (`MeshRef`, `MaterialRef`, a UI font or image) into
handles — files what it lacks and resolves the entity on a later frame, so no
frame waits for the disk. A failed decode is logged once, as a warning, and the
entity waits for the file to change; there is no fallback asset.

Loading is a **service**, and decoding is a **family**. The service has nothing
to choose: an asset is in its cache or it is not. The decoding work, though, is
done by the `loading` family's lane, which runs under the budget like every other
family (see [Agents and Lanes](./agents-and-lanes.md)), so a burst of decodes
competes for the frame instead of stealing it.

The decoder set grows by registration: a new format is a decoder registered
under a type name, not a change to the chain (see
[How-to: add an asset decoder](../how-to/add-an-asset-decoder.md)). The same holds
for the I/O: another source is another `AssetIo` behind the same index and
decoders.

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

The list is deduplicated and sorted, so the index is the same bytes from run to
run. The other types return an empty list — scenes, prefabs and meshes among
them. A scene holding [prefab instances](./prefabs.md) keeps each as a link to
the prefab's uuid, and that link is **not** recorded as a dependency: the
serialization service reads each prefab when it expands the scene, but the index
cannot tell which scenes use a prefab. Adding an extractor is one match arm. To
keep the scan fast, bytes are read only for a type that has an extractor:
textures and sounds are never opened.

## When an asset changes

The editor watches the project's files. When one changes, it invalidates the
asset in the service (`AssetService::invalidate`), which drops the cached copy and
announces the change; handles already out keep the old value. The
**`asset_reload`** DataSystem, at the start of `PreExtract`, takes the announced
changes and requests a fresh copy of every changed asset an entity holds:

- until the new copy lands, the last good copy keeps serving;
- once it lands, it replaces the old one under the same uuid, and the GPU copy
  made from the old one is retired (see [Residency](./residency.md));
- a file that no longer decodes — caught mid-write — keeps the last good copy
  until it changes again;
- an asset gone from the index is dropped from every entity that held it.

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

> *uuid(p)* = *R(key(p))* if the registry holds *key(p)*, else *v5(N_asset, key(p))*,

and a part labelled *ℓ* of a container *c* has *v5(N_sub, bytes(c) ‖ ℓ)*. The two
namespaces are distinct constants, so a part's uuid never equals a path's.
*uuid* depends only on *key(p)* and *R*, never on the scan's order or the system
it runs on; the index sorts its entries by key, so the index of one folder and
one registry is the same on every run.

**The request contract.** For one uuid and one type, `request` files at most one
decode while one is pending. Between two asset changes it answers `Failed` for a
uuid whose last decode failed, so a system resolving every frame does not retry a
broken file every frame; any change clears it.

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
