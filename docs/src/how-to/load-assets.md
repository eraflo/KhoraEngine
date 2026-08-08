# Load and reference assets

This guide shows you how to reference a mesh, a texture, and a sound from your project's
assets — and how the same code works for loose files in development and a packed archive
in a release build.

> **Prerequisites.** You can spawn entities and attach components
> ([Spawn entities and move them](./spawn-and-transform.md)).

## How assets are identified

Every asset has a stable `AssetUUID`, decoupled from its file path. By **default** the
asset index derives that UUID from the asset's forward-slash relative path with
`AssetUUID::new_v5`:

```rust
use khora_sdk::prelude::AssetUUID;

let wood_uuid = AssetUUID::new_v5("textures/wood.png");
```

The default is only a starting point. The moment you rename or move an asset in the
editor, its identity is **frozen** in the project's identity registry
(`<project>/.khora/asset-registry.ron`), so the UUID stays put even though the path
changed — every `MeshRef::Asset`, `MaterialRef`, and texture slot that referenced it
keeps resolving, with nothing to rewrite. See
[File formats — asset identity registry](../reference/formats.md#asset-identity-registry).

Either way the resolved UUID is **identical** in development (loose files via the
`FileLoader`) and in release (a single `data.pack` via the `PackLoader`), because both
resolve through the same registry. Game code that carries a UUID or an
`AssetHandle<T>` does not change between the two — see
[Assets and the VFS](../concepts/assets.md) for the pipeline.

## Reference a mesh

The quickest path is a procedural primitive — `spawn_plane` / `spawn_cube_at` /
`spawn_sphere` attach a content-derived `MeshRef::Procedural` for you (identical
primitives dedup to one GPU mesh):

```rust
let ball = khora_sdk::spawn_sphere(world, 0.75, 32, 16).build();
```

To reference an imported mesh asset (glTF / OBJ) by UUID, attach `MeshRef::Asset`:

```rust
use khora_sdk::prelude::AssetUUID;
use khora_sdk::khora_data::ecs::MeshRef;

let ship = khora_sdk::Vessel::new(world)
    .with_component(MeshRef::Asset(AssetUUID::new_v5("models/ship.gltf")))
    .build();
```

The resolver loads and uploads the referenced asset before the GPU mesh projection runs.

## Reference a texture on a material

A material references a texture by UUID. Set `base_color_texture` (or
`metallic_roughness_texture`, `normal_map`, `emissive_texture`) on a `StandardMaterial`:

```rust
use khora_sdk::prelude::AssetUUID;
use khora_sdk::prelude::materials::StandardMaterial;
use khora_sdk::prelude::math::LinearRgba;

let mat = StandardMaterial {
    base_color: LinearRgba::WHITE,
    base_color_texture: Some(AssetUUID::new_v5("textures/wood.png")),
    roughness: 0.6,
    ..Default::default()
};
let handle = world.add_material(mat);
khora_sdk::spawn_cube_at(world, khora_sdk::prelude::math::Vec3::ZERO, 1.0)
    .with_component(handle)
    .build();
```

## Load a sound

Sounds decode to `SoundData` (`khora_sdk::SoundData`). An `AudioSource` component holds
an `AssetHandle<SoundData>`; how you obtain that handle and play it spatially is covered
in [Play 3D audio](./play-3d-audio.md).

## Load through the `AssetService` (VFS-backed)

For assets registered in the project's VFS, the `AssetService` resolves a UUID through
**VFS → IO → decode → store** and returns a reference-counted handle. It is registered as
an engine resource — cache it in `setup`:

```rust
use std::sync::Arc;
use khora_sdk::AssetService;
use khora_sdk::prelude::AssetUUID;
use khora_sdk::renderer::scene::Mesh;

// In `setup`, with `runtime: &Runtime`:
if let Some(service) = runtime.services.get::<Arc<std::sync::Mutex<AssetService>>>() {
    if let Ok(mut svc) = service.lock() {
        let mesh = svc.load::<Mesh>(&AssetUUID::new_v5("models/ship.gltf"));
        // `mesh` is `Result<AssetHandle<Mesh>>`; handle the error, don't unwrap on IO.
    }
}
```

`load::<A>(&uuid)` is synchronous and caches: a second `load` of the same UUID returns the
cached handle. `AssetHandle<T>` is cheap to clone (an `Arc`), so many entities can share
one asset without duplicating GPU memory.

> **Dev vs. packed.** The editor's *Build Game* bundles `assets/` into `data.pack` +
> `index.bin`. The runtime opens the pack instead of loose files; UUIDs and handles are
> unchanged, so no game code edits are needed for a release build.

## Expected result

A mesh referenced by UUID renders once its asset resolves; a material's texture UUID is
uploaded and sampled as the albedo map. A missing UUID is reported (the mesh or texture
is skipped) rather than crashing the frame.

## Related

- [Materials and lighting](./materials-and-lighting.md) — attach the loaded texture.
- [Play 3D audio](./play-3d-audio.md) — use a loaded `SoundData`.
- [Assets and the VFS](../concepts/assets.md) — UUIDs, decoders, pack archives.
