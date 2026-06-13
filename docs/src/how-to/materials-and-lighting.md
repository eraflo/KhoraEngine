# Apply materials and light a scene

This guide shows you how to give a mesh a PBR material and light it with directional
and point lights.

> **Prerequisites.** You can spawn meshes
> ([Spawn entities and move them](./spawn-and-transform.md)). Every lit mesh needs an
> explicit material — the projection has no default-material fallback, so a mesh without
> a material handle is skipped (and logged).

## Create a `StandardMaterial` and attach it

`StandardMaterial` is the metallic-roughness PBR material. Build one with the fields you
care about and fall back to `Default` for the rest, then turn it into a handle with
`world.add_material` and attach it to a mesh entity with `with_component`.

```rust
use khora_sdk::prelude::materials::StandardMaterial;
use khora_sdk::prelude::math::LinearRgba;

let red_metal = StandardMaterial {
    base_color: LinearRgba::new(0.9, 0.1, 0.1, 1.0),
    metallic: 1.0,
    roughness: 0.2,
    ..Default::default()
};
let handle = world.add_material(red_metal);

khora_sdk::spawn_sphere(world, 0.75, 32, 16)
    .at_position(khora_sdk::prelude::math::Vec3::new(0.0, 0.5, -5.0))
    .with_component(handle)
    .build();
```

`add_material` returns a `MaterialRef::Inline` that embeds the material in the scene; the
resolver and GPU projection turn it into a runtime material. To reference a packaged
`.kmat` asset instead, attach `MaterialRef::Asset(uuid)` — see
[Load and reference assets](./load-assets.md).

### The PBR knobs

| Field | Range | Effect |
|---|---|---|
| `base_color` | `LinearRgba` | Albedo (diffuse for dielectrics, reflectance for metals). |
| `metallic` | `0.0` / `1.0` | Dielectric vs. metal. Intermediate values are non-physical. |
| `roughness` | `0.0`–`1.0` | Mirror-sharp (`0.0`) to fully diffuse (`1.0`) reflections. |
| `emissive` | `LinearRgba` | Self-illumination, added after lighting. |
| `base_color_texture` | `Option<AssetUUID>` | Albedo map, multiplied with `base_color`. |
| `double_sided` | `bool` | Render back faces instead of culling them. |

To texture the albedo, set `base_color_texture: Some(uuid)` — covered in
[Load and reference assets](./load-assets.md).

## Add a directional light (the sun)

`Light::directional()` makes a sun-like light. Its direction comes from the entity's
rotation; tilt the entity to aim it. Reach into `light_type` to set intensity and enable
shadows.

```rust
use khora_sdk::prelude::ecs::{Light, LightType};
use khora_sdk::prelude::math::{Quaternion, Vec3};

let mut sun = Light::directional();
if let LightType::Directional(ref mut d) = sun.light_type {
    d.intensity = 2.5;
    d.shadow_enabled = true;
    d.shadow_bias = 0.005;
    d.shadow_normal_bias = 0.02;
}

khora_sdk::Vessel::at(world, Vec3::new(0.0, 20.0, 5.0))
    .with_component(sun)
    .with_rotation(Quaternion::from_axis_angle(
        Vec3::X,
        -std::f32::consts::FRAC_PI_2 * 0.8,
    ))
    .build();
```

## Add a point light

`Light::point()` is an omni light positioned by the entity's transform, with a `color`,
`intensity`, and `range`. `intensity` is a direct linear multiplier paired with a
windowed distance attenuation, so a few units is already bright.

```rust
use khora_sdk::prelude::ecs::{GlobalTransform, Light, LightType, Transform};
use khora_sdk::prelude::math::{LinearRgba, Vec3};

let mut lamp = Light::point();
if let LightType::Point(ref mut p) = lamp.light_type {
    p.intensity = 5.0;
    p.color = LinearRgba::new(0.8, 0.9, 1.0, 1.0);
    p.range = 15.0;
}

world.spawn((
    Transform::from_translation(Vec3::new(0.0, 1.0, -2.0)),
    GlobalTransform::default(),
    lamp,
));
```

`Light::spot()` works the same way, exposing `inner_cone_angle` / `outer_cone_angle`
through `LightType::Spot`.

## Expected result

The sphere renders shaded by the sun and tinted by the point light. With
`shadow_enabled = true` on the directional light, meshes cast shadows onto the ground
plane. A mesh with no material is silently skipped — if a shape is invisible, check that
you attached a handle.

## Related

- [Load and reference assets](./load-assets.md) — albedo / normal / emissive textures.
- [Rendering and PBR](../concepts/rendering.md) — the shading model and shadow pipeline.
- [`StandardMaterial` and light reference](../reference/sdk.md) — every field and default.
