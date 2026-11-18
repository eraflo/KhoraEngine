# Scene / Design — reference

Domain knowledge for scenes and presentation. Consulted during Research (dispatch the `codebase-*`
subagents to apply it). For any visual/design decision, use `/impeccable`. Reference:
[`../sdk-guide.md`](../sdk-guide.md).

## Scope
Composing scenes in the editor (entities, lights, cameras), materials, environment and sky, prefabs,
HUD and menu UI, loading and saving scenes.

## Key API
- **Editor**: Scene Tree (*Add*), Inspector (components, materials, a behavior's fields), Asset Browser
  (drag a `.kprefab` to place an instance), `Ctrl+S` to save the `.kscene`.
- **Lights**: `Light::directional()` / `Light::point()` / `Light::spot()`, configured through
  `LightType` (intensity, range, `shadow_enabled`, bias).
- **Materials**: `StandardMaterial` (PBR `base_color`, `roughness`, `metallic`, `alpha_mode`),
  `UnlitMaterial`, `EmissiveMaterial`, `WireframeMaterial`, `AlphaMode`. A mesh without a material is
  not drawn. A script swaps one with `e.SetMaterial(m)`.
- **Cameras**: `Camera::new_perspective(fov_y, near, far)` — no aspect: it is the shape of the window
  (or viewport) the camera fills, read every frame. A value the engine cannot use (a near plane at 0)
  draws nothing and is reported, never panics.
- **Environment**: `EnvironmentMap::from_asset(uuid)` inserted in `provide` selects the IBL map; without
  it the procedural sky is baked.
- **Prefabs**: `.kprefab` files; instances stay linked, overrides per field; *Apply* writes the prefab.
- **Scenes in code**: `SerializationService` + `SerializationGoal` (the encoding, never the content); see
  [`load-scene`](../skills/load-scene/SKILL.md).

## Rules
- **For every visual / UI / design decision — typography, color, spacing, layout, motion — use
  `/impeccable`** (audit / critique / polish).
- A scene holds what was authored; play-made state goes in game saves, never in the scene.
- Math via `prelude::math`; Y-up, right-handed. No `unwrap()` on asset I/O. No secrets in builds.

## Skills
- **`/impeccable`** — the primary design tool for HUD, menus, and scene presentation:
  - `/impeccable audit <area>` · `/impeccable critique <area>` · `/impeccable polish <area>`.
- [`spawn-entity`](../skills/spawn-entity/SKILL.md) — place props, lights, cameras.
- [`load-scene`](../skills/load-scene/SKILL.md) — load/save the scene.

Verify with Play — the scene renders, lighting and shadows look right — then `/impeccable audit` the UI.
