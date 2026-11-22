# Residency

<p class="kp-lede">A picture in a book on a shelf is no use to a painter until it is on the easel in front of them. A game's assets are the same: a texture decoded in memory cannot be drawn until a copy of it sits on the graphics card. Residency is how Khora keeps that copy where it is used, makes it once, makes it again when the picture changes, and gives the space back when nothing shows it any more.</p>

This page explains what a residency is, which ones the engine has, when they run
in a frame, what they do with a device that says no, and how the copies they made
are released. For how an asset is found and decoded before it gets here, see
[Assets](./assets.md).

---

## What residency is

A device works on its own memory. A graphics processor (GPU) draws a mesh from
vertex buffers in its memory, and a UI image from a rectangle of a texture it
holds. The asset service gives the engine a **CPU copy** of each asset: a `Mesh`,
a `CpuTexture`. Something has to turn that copy into what the device uses, keep
the two in step while the game runs, and free the device's copy when it is no
longer needed.

A **residency** is that something, for one kind of asset on one kind of device. It
names three things:

- its **source**, the CPU asset it starts from (`Mesh`, `CpuTexture`);
- its **resident**, what it makes on the device: handles to buffers, or a
  rectangle of an atlas, cheap to copy;
- its **target**, what it makes it on for one frame: the device itself, or a
  pool on it such as the UI atlas.

The contract is the `Residency` trait (`khora-core/src/residency/mod.rs`):

```rust
pub trait Residency: Send + Sync + 'static {
    const NAME: &'static str;        // its DataSystem's name
    const DEVICE: DeviceClass;       // Cpu, Gpu, Npu or Other
    type Source: Asset;
    type Resident: Clone + Send + Sync;
    type Target<'a>;

    fn make(source: &Self::Source, target: &mut Self::Target<'_>)
        -> Result<Self::Resident, ResidencyError>;
    fn release(resident: Self::Resident, target: &mut Self::Target<'_>);
}
```

A residency knows how to make a resident and how to give one back. It does not
know which copies it already made, nor when an asset changed: its **store**
does.

## The residencies the engine has

Three live in `khora-data/src/residency/`, all on the GPU:

| Residency | Source → resident | Target | Store |
|---|---|---|---|
| `image_on_gpu` (`ImageOnGpu`) | `CpuTexture` → `AtlasRect` | `AtlasTarget`: the device and the UI atlas | `Resident<ImageOnGpu>`, held by the `UiImageAtlas` resource |
| `gpu_mesh_sync` (`MeshOnGpu`) | `Mesh` → `AssetHandle<GpuMesh>` | the `GraphicsDevice` | `Assets<GpuMesh>` in the `AssetStore`, through the `ProjectionRegistry` |
| `gpu_material_sync` | a material → `GpuMaterial` | the device and the pipeline system | `Assets<GpuMaterial>` in the `AssetStore` |

- `image_on_gpu` places every image a `UiImage` names in the UI atlas, and is
  the one built entirely on the store described below.
- `gpu_mesh_sync` implements the trait for the upload and keeps its residents in
  the asset store, where the render lanes read them. Each entity whose mesh is
  resident is tagged with a `HandleComponent<GpuMesh>`; a mesh shared by many
  entities is uploaded once.
- `gpu_material_sync` is registered as a residency but does not implement the
  trait: a material's upload needs the pipeline layout of its variant and every
  texture it names decoded first. It uploads the material's uniforms and
  textures, builds its bind group against the layout the lit pipeline binds for
  that variant, and tags the entity with a `HandleComponent<GpuMaterial>`. It
  runs after the meshes.

The glyphs of UI text are made resident by `glyphs_on_gpu`, a `DataSystem` that
is not a residency: a glyph is not an asset.

## One asset, from file to device

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../images/residency/reach-the-device.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>A name.</strong> An entity's <code>UiImage</code> holds the image's uuid. Nothing of the file is in memory yet.</li>
<li><strong>Decoded off the frame.</strong> The resolver asks the asset service; the <code>loading</code> family decodes the file away from the frame, and the <code>CpuTexture</code> lands in the asset store at a frame boundary.</li>
<li><strong>Made resident.</strong> At <code>PreExtract</code>, <code>image_on_gpu</code> asks its store to ensure the image. Nothing is kept under its uuid, so the residency places it in the atlas; the store keeps the rect beside the copy it was made from.</li>
<li><strong>Read, and kept.</strong> The UI lane samples the atlas at that rect. Later frames find the same copy and upload nothing.</li>
<li><strong>The file changes.</strong> The reload puts a new copy in the asset store under the same uuid. The store sees another copy: it makes the image again and releases the old rect.</li>
<li><strong>Refused.</strong> The atlas has no room. It is built again, once in that run, from the images in use; an image that still does not fit is logged once and waits until an image in use goes away. The last good rect of an image too large for any atlas stays.</li>
</ol>
</figure>

## When residencies run

A residency runs as a `DataSystem` in the `PreExtract` phase: after the game's
update and the hierarchy fix-ups, before the Flows project the world and before
any agent runs. Whatever a lane draws this frame was made resident this frame.

<div class="kp-figure-frame">

{{#include ../images/residency/in-the-frame.svg}}

</div>

`register_residency!` (`khora-data/src/residency/mod.rs`) writes the
registration:

```rust
register_residency!(name = ImageOnGpu::NAME, run = image_on_gpu_system);
register_residency!(
    name = "gpu_material_sync",
    run = gpu_material_sync_system,
    after = ["gpu_mesh_sync"],
);
```

It submits a `DataSystemRegistration` in `TickPhase::PreExtract`, with the role
`Upkeep` (it runs whichever families are on), ordered after `asset_reload` and
`asset_resolver` and after whatever `after` names. Those two run first in the
phase for a reason:

- `asset_reload` replaces the copies of every asset whose file changed, so a
  changed asset reaches the device the frame its new copy landed;
- `asset_resolver` turns the references entities hold (`MeshRef`,
  `MaterialRef`, a UI font or image) into handles, so a residency sees this
  frame's references.

The copies they read are already there. A decode filed off the frame lands at
the frame's very first phase, `Boundary`, where `asset_intake` takes the decoded
assets into the service and the asset store, before the simulation and well
before `PreExtract`. Inside `PreExtract`, the systems run in the order their `runs_after` lists
give, ties broken by their `order_hint` and then their name: `asset_reload`
(hint −6), then `asset_resolver` (−5), then the residencies (0), with
`gpu_material_sync` after `gpu_mesh_sync`. A `runs_after` naming a system of
another phase is ignored: phases already order those.

A residency needs its device: without a `GraphicsDevice` in the runtime (a
headless run), each of the three returns at once and makes nothing.

## How a residency is fed

A residency reads two things, and writes only its store and its tags.

**The assets.** Its sources are the CPU copies in the `AssetStore`, one per uuid.
A copy is an `AssetHandle`, a shared pointer: two handles are the same copy only
if they point at the same value. The reload never edits a copy in place; it
stores a new one under the same uuid. That is what lets a store tell "already
made" from "changed".

**The world's changes.** Which assets are wanted comes from the entities:

- `image_on_gpu` collects, each frame, the textures every `UiImage` names;
- `gpu_mesh_sync` visits every entity holding a `HandleComponent<Mesh>` but no
  `HandleComponent<GpuMesh>` yet;
- `gpu_material_sync` visits every entity holding a resolved material but no
  `HandleComponent<GpuMaterial>` yet.

So an entity that gains a mesh is uploaded at the next `PreExtract`, and an entity
already tagged costs nothing. When a mesh or a material changes, the reload takes
its GPU copy out of the store and removes the entity's tag: the next residency run
sees an untagged entity and uploads the new copy.

## The store: made once per copy

`Resident<R>` (`khora-core/src/residency/mod.rs`) is the store of one residency:
for each asset uuid, one resident and the copy of the source it was made from (a
clone of its handle, so that copy stays alive and no later copy can be mistaken
for it).

<div class="kp-figure-frame">

{{#include ../images/residency/ensure.svg}}

</div>

| Call | Does |
|---|---|
| `ensure(key, source, target)` | returns the resident made from this copy, making it on `target` if the entry was made from another copy or none; a new resident replaces the stale one, which is released |
| `get(key)` | the resident kept under `key`, whatever copy it was made from |
| `insert(key, resident)` | keeps a resident placed by hand, with no copy behind it: the next `ensure` makes it again from its source |
| `evict(key, target)` | removes the resident and releases it |
| `evict_all(target)` | removes every resident and releases each, for a target made again whole |
| `source(key)` | the copy the resident under `key` was made from; none for one placed by hand |

Hot reload is the store's job, not each residency's: a residency only says how to
make and release. A lane reads a residency's store as an input of its own, a
`#[resident]` field of type `&'f Resident<R>`, which its contention declares as a
read (see [Agents and Lanes](./agents-and-lanes.md)); the store is then a runtime
resource. The UI lane reads the atlas through the `UiImageAtlas` resource, which
holds the store.

## Refusals and retries

A device can say no: out of memory, a size it does not support, an atlas full. A
residency never panics on that and never substitutes a placeholder. It says
no in a `ResidencyError` (the residency's name and the reason in words) and the
caller decides what is kept.

| What is refused | What happens |
|---|---|
| An image the atlas has no room for | The atlas is built again from the images in use, at most once a run (see below). An image the rebuilt atlas still has no room for is left out, logged once as a warning, and tried again once an image in use then goes away or is replaced. |
| An image larger than an empty atlas | No rebuild could hold it, so none is made. Its last good copy is drawn instead, across rebuilds. The refusal is logged once; a new copy is tried on the next frame. |
| A mesh's vertex buffer | The mesh is not cached and the entity stays untagged, logged as an error. The next frame tries again; once the device accepts, the mesh is uploaded and tagged. |
| A mesh's index buffer | The same, and the vertex buffer already made for it is given back to the device first. |
| The material sampler | No material is uploaded while the device refuses it, logged as an error. The sampler is made once, the first time the device accepts it, and shared by every material. |
| A material texture or bind group | That material is not uploaded this run, after a logged error. The next run tries it again. |

What is *missing* is not a refusal. A material whose textures are not decoded yet
waits, with no warning, until they land. A mesh with no material reference is
logged and skipped: there is no default material.

## Eviction

A resident costs device memory for as long as it is kept. The meshes and
materials are released by the **`AssetEviction`** service
(`khora-data/src/gpu/eviction.rs`), ticked by the `asset_eviction` DataSystem in
`Maintenance`, at the end of the frame:

- **Retired** residents — the GPU copies the reload took out because their
  asset changed — are freed at its next tick, at the end of the frame, after the
  render that may still read them.
- **Orphans** — GPU meshes and materials no live entity's handle references —
  are freed up to 16 a frame, materials first, so a burst of despawns spreads
  its teardown over several frames. Retired residents do not count against the
  16.

A despawned entity whose page is not compacted yet keeps no asset alive: the query
eviction uses skips orphan rows. Eviction removes only what nothing references,
so it never changes what the game shows.

### The UI atlas: rebuilt, never freed rect by rect

The UI atlas packs rects one after the other and cannot free one: `release` of
an `ImageOnGpu` gives nothing back, and the rect of a replaced or no longer shown
image is left unused. That room comes back when the atlas runs out of it. The
first image of a run that does not fit makes `image_on_gpu` build the atlas
again, as the glyph atlas does.

One rule says what the atlas draws. An image is **in use** when a live
`UiImage` names it, whether or not the asset store holds a copy of it. For each
image in use, the copy drawn is its current copy when that copy fits an empty
atlas, and otherwise its **last good copy**: the one it was left out with, else
the one its rect was made from. The last good copy also stands in while the
store holds no copy, as while a reload decodes. An image with neither draws
nothing. The rebuild:

1. chooses the copy each image in use draws, before anything is evicted;
2. makes a fresh texture, evicts every rect of the old atlas, and destroys the
   old texture and its view. The UI lane sees the new view and binds it in place
   of the old;
3. uploads the chosen copies into the fresh texture, in the order the world
   names the images, each at a new rect;
4. leaves out the images it has no room for, each with the copy it tried: that
   copy stays its last good copy, so a current copy too large for any atlas
   never replaces it. They are logged once, and not tried again, nor the atlas
   rebuilt, until an image drawn then goes away or draws another copy: only that
   makes room. A new current copy that fits is tried at once.

A run rebuilds the atlas at most once, however many images it refuses. An image
larger than an empty atlas never causes a rebuild.

## Write a residency

1. Implement `Residency` for a unit type: its `NAME` (its DataSystem's name),
   its `DEVICE`, its source, resident and target, `make` (fallible) and
   `release`.
2. Hold a `Resident<R>` where its readers find it, and write a system that
   collects the uuids wanted this frame, fetches their copies from the asset
   store and calls `ensure`. Decide what a refusal costs: keep the last good
   resident, log once.
3. Register it with `register_residency!`.
4. Free what nothing uses: call `evict`, or leave it to a maintenance service
   like `AssetEviction`.

`ImageOnGpu` (`khora-data/src/residency/image_on_gpu.rs`) is the example to copy.

## Under the hood

**Notation.** For one residency *R* and one key *k*, write *A_t(k)* for the copy the
asset store holds at frame *t*, and *S(k)* for the copy the store's entry was made
from (⊥ when the entry is absent, or placed by hand). Copies compare by identity:
*a = b* when both handles point at one value.

**Invariant.** After `ensure(k, a)` returns `Ok(r)`, the entry under *k* is
*(a, r)*: *S(k) = a* and *r* is the value `make(a)` returned. After it returns
`Err`, the entry is what it was before the call.

**Theorem (one make per copy).** Let `ensure(k, ·)` be called with copies
*a₁, a₂, …, aₙ*, and let every `make` succeed. Then `make` is called exactly once
per maximal run of equal consecutive copies, and `release` once per make after the
first.

*Proof.* By the invariant, after the call with *aᵢ*, *S(k) = aᵢ*. The call with
*aᵢ₊₁* makes nothing when *aᵢ₊₁ = aᵢ* (the first branch returns the kept
resident), and otherwise calls `make` once, inserts *(aᵢ₊₁, r)* and releases the
resident it replaced. The first call finds no entry and makes once, releasing
nothing. Counting the changes of value along the sequence gives the result. ∎

A failed `make` leaves the entry as it was, so the theorem holds on the successful
calls: a refused copy is tried again on the next call with the same copy, and
nothing is released.

**Bounds.** A frame's eviction frees every resident retired since its last tick,
plus at most *m* = 16 orphans. With no new orphan, a burst of *N* orphans is freed in
⌈*N / m*⌉ frames. A mesh shared by *E* entities costs one upload, not *E*: the
cache is keyed by the asset's uuid.

**Rebuilds of the UI atlas.** Let *U_t* be the set of pairs (uuid, copy drawn) of
the images in use at run *t*, and *L_t* the images left out after it. Run *t* rebuilds the atlas at
most once: the flag set by the first rebuild turns every later refusal of that
run into a left-out image. With *L_{t−1}* ≠ ∅, run *t* rebuilds for it only when
*U_{t−1}* ⊄ *U_t*, that is when an image in use went away or draws another copy, and
otherwise only when a copy not left out finds no room. So with a fixed *U*, the
atlas is rebuilt at most once in a row of runs, however long: after it, every
image is placed or left out, and neither retries. The rect an image is drawn
from always lies in the texture made by the last rebuild: a rebuild evicts every
rect before it places any, and it chooses the copies before it evicts, so no
last good copy is lost with the rects.

**Why a retired copy is not freed at once.** The reload runs in `PreExtract`; the
frame's render, later in the same frame, may have recorded commands that read
the old buffers. Freeing them at the frame's `Maintenance` tick, after that
render, keeps every command's buffers alive for as long as it reads them.
