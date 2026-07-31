# Scene / Design — reference

Domain knowledge for scene and presentation on `khora-sdk`. Consulted during Research (dispatch the
`codebase-*` subagents to apply it). For any visual/design decision, use `/impeccable`. Reference:
[`../sdk-guide.md`](../sdk-guide.md).

## Scope
Composing scenes (layout of entities, lights, cameras), materials, ground/skybox, HUD and menu UI, and
loading/saving scenes.

## Key API
- Lights: `Light::directional()` / `Light::point()` / `Light::spot()`, configure via `LightType`
  (intensity, range, `shadow_enabled`, bias).
- Materials: `StandardMaterial` (PBR base_color/roughness/metallic), `UnlitMaterial`, `EmissiveMaterial`.
- Cameras: `Camera::new_perspective(fov, aspect, near, far)`.
- Scenes: `SceneFile` + `SerializationGoal`; author visually with `cargo run -p khora-editor`.

## Rules
- **For every visual / UI / design decision — typography, color, spacing, layout, motion — use
  `/impeccable`** (audit / critique / polish).
- SDK-only; math via `prelude::math`; Y-up, right-handed. No `unwrap()` on asset IO. No secrets in builds.

## Skills
- **`/impeccable`** — the primary design tool for HUD, menus, and scene presentation:
  - `/impeccable audit <area>` · `/impeccable critique <area>` · `/impeccable polish <area>`.
- [`spawn-entity`](../skills/spawn-entity/SKILL.md) — place props, lights, cameras.
- [`load-scene`](../skills/load-scene/SKILL.md) — load/save the scene.

Verify with `cargo run` — scene renders, lighting/shadows look right — then `/impeccable audit` the UI.
