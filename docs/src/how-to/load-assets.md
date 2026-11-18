# Load and reference assets

This guide shows you how to reference a mesh, a texture and a sound from your
project's assets, and how to load one from code — with the same code for loose
files in development and a packed archive in a shipped game.

> **Prerequisites.** You can spawn entities and attach components
> ([Spawn entities and move them](./spawn-and-transform.md)).

## How assets are identified

Every asset has a stable `AssetUUID`, independent of its file path. By default the
asset index derives it from the asset's path relative to `assets/`, with forward
slashes:

```rust
use khora_sdk::prelude::AssetUUID;

let wood = AssetUUID::new_v5("textures/wood.png");
```

The default is only a starting point. The moment you rename or move an asset in
the editor, its uuid is **frozen** in the project's identity registry
(`<project>/.khora/asset-registry.ron`): the uuid stays while the path changes,
and every `MeshRef::Asset`, `MaterialRef` and texture slot naming it keeps
resolving. See
[File formats — asset identity registry](../reference/formats.md#asset-identity-registry).

The uuid is the same in development (loose files read by a `FileLoader`) and in a
shipped game (one `data.pack` read by a `PackLoader`), because both resolve through
the same registry. See [Assets and the VFS](../concepts/assets.md).

## Reference a mesh

The quickest path is a procedural primitive: `spawn_plane`, `spawn_cube_at` and
`spawn_sphere` attach a `MeshRef::Procedural` whose uuid is derived from its
parameters, so identical primitives share one GPU mesh:

```rust
let ball = khora_sdk::spawn_sphere(world, 0.75, 32, 16).build();
```

To reference an imported mesh (glTF, OBJ) by its uuid, attach `MeshRef::Asset`:

```rust
use khora_sdk::khora_data::ecs::MeshRef;
use khora_sdk::prelude::AssetUUID;

let ship = khora_sdk::Vessel::new(world)
    .with_component(MeshRef::Asset(AssetUUID::new_v5("models/ship.gltf")))
    .build();
```

That is all the entity needs. Each frame, the engine's **asset resolver** asks the
asset service for what an entity names and does not hold yet. The file is decoded
off the frame by the `loading` family; on a later frame the entity gets its mesh,
and the mesh residency uploads it to the GPU before anything draws (see
[Residency](../concepts/residency.md)). Until then the entity simply draws nothing:
no frame waits for the disk.

A glTF file's uuid draws its whole default scene as one mesh. To draw one of its
primitives alone, name the part by its label inside the file (a material part
goes in `MaterialRef::Asset` the same way):

```rust
use khora_sdk::khora_data::ecs::MeshRef;
use khora_sdk::prelude::AssetUUID;

let ship_file = AssetUUID::new_v5("models/ship.gltf");
let hull = AssetUUID::sub_asset(ship_file, "mesh0/prim0"); // mesh 0, primitive 0
let hull_entity = khora_sdk::Vessel::new(world)
    .with_component(MeshRef::Asset(hull))
    .build();
```

The labels (`scene`, `mesh{m}/prim{p}`, `material{i}`, `texture{i}`, …) are listed
in [Assets — glTF files](../concepts/assets.md#gltf-files). To place a glTF's
scene as a tree of entities, place it as a prefab from the editor.

## Reference a texture on a material

A material names its textures by uuid. Set `base_color_texture` (or
`metallic_roughness_texture`, `normal_map`, `emissive_texture`) on a
`StandardMaterial`:

```rust
use khora_sdk::prelude::AssetUUID;
use khora_sdk::prelude::materials::StandardMaterial;
use khora_sdk::prelude::math::{LinearRgba, Vec3};

let mat = StandardMaterial {
    base_color: LinearRgba::WHITE,
    base_color_texture: Some(AssetUUID::new_v5("textures/wood.png")),
    roughness: 0.6,
    ..Default::default()
};
let material = world.add_material(mat); // a MaterialRef, saved inside the scene
khora_sdk::spawn_cube_at(world, Vec3::ZERO, 1.0)
    .with_component(material)
    .build();
```

`add_material` returns an inline `MaterialRef`: the material is saved inside the
scene. To reference a `.kmat` asset instead, attach `MaterialRef::Asset(uuid)`.
The material is uploaded once every texture it names is decoded.

## Load a sound

Sounds decode to `SoundData` (`khora_sdk::SoundData`). An `AudioSource` component
holds an `AssetHandle<SoundData>`; how you obtain that handle and play it in space
is covered in [Play 3D audio](./play-3d-audio.md).

## Load from code through the `AssetService`

To hold an asset yourself — a sound to play on demand, a level's data — ask the
**`AssetService`**. The engine keeps it in the runtime's services:

```rust
use std::sync::{Arc, Mutex};
use khora_sdk::AssetService;
use khora_sdk::prelude::AssetUUID;
use khora_sdk::renderer::gpu_scene::Mesh;

// In a hook that receives the runtime, such as `after_agents`:
if let Some(service) = runtime.services.get::<Arc<Mutex<AssetService>>>() {
    if let Ok(mut svc) = service.lock() {
        match svc.load::<Mesh>(&AssetUUID::new_v5("models/ship.gltf")) {
            Ok(mesh) => { /* keep the AssetHandle<Mesh> */ }
            Err(error) => log::warn!("the ship did not load: {error}"),
        }
    }
}
```

`load::<A>(&uuid)` reads and decodes on the calling thread and caches: a second
`load` of the same uuid returns the cached handle. It fails when the uuid is not
indexed, its bytes cannot be read or are refused, or they do not decode as `A` —
handle the error, never `unwrap` it. An `AssetHandle<T>` is cheap to clone (a
shared pointer), so many entities share one copy.

**When the index is there.** A shipped game opens its pack's index before
`setup`. A game started from loose files indexes `assets/` off the frame, once
the engine runs: during `setup` its service has no asset yet, and `load` answers
that the uuid is not indexed. Load from a hook called once frames run, after your
application's `startup_ready` was called — the startup waits for the index.

`load` blocks the frame it runs in for as long as the disk and the decoder take.
For a large asset, prefer referencing it from a component and letting the resolver
request it.

> **Dev vs. packed.** The editor's *Build Game* writes `data.pack`, `index.bin` and
> a signed `manifest.bin`, and stamps the game's binary with the key that checks
> them. The shipped game opens the pack instead of loose files, and refuses a pack
> whose seal does not verify. Uuids and handles are unchanged: no game code
> changes for a release build.

## Expected result

A mesh referenced by uuid renders a few frames after the entity is spawned, once
its file is decoded and uploaded; a material's texture is sampled as soon as the
material is uploaded. A uuid that does not resolve, or a file that does not decode,
is logged once and leaves that entity without the asset — the frame goes on.

## Related

- [Materials and lighting](./materials-and-lighting.md) — attach the loaded texture.
- [Play 3D audio](./play-3d-audio.md) — use a loaded `SoundData`.
- [Assets and the VFS](../concepts/assets.md) — uuids, decoders, pack archives.
- [Residency](../concepts/residency.md) — how a decoded asset reaches the GPU.
