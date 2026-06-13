# How-to guides

Task-sized recipes. Each page solves **one** goal with real, verified APIs and a
way to check it worked. These assume you've done the
[Your first game](../tutorials/your-first-game.md) tutorial (for game tasks) or
[Extending the engine](../tutorials/extending-the-engine.md) (for engine tasks) —
they don't re-teach the basics, they link to them.

Looking for the "why" behind a recipe? Follow the links into [Concepts](../concepts/saa.md).
Looking for the exact signature? See the [API reference](../reference/api.md).

## Building a game

| I want to… | Recipe |
|---|---|
| Spawn entities and move them | [Spawn and transform entities](./spawn-and-transform.md) |
| Give surfaces materials and light a scene | [Materials and lighting](./materials-and-lighting.md) |
| Load a mesh, texture, or sound | [Load assets](./load-assets.md) |
| Play positional 3D audio | [Play 3D audio](./play-3d-audio.md) |
| Lay out a UI | [Build a UI](./build-a-ui.md) |
| Save and restore the world | [Save and load scenes](./save-and-load-scenes.md) |
| Bind input to actions | [Map input](./map-input.md) |

## Extending the engine

| I want to… | Recipe |
|---|---|
| Define and register an ECS component | [Add a component](./add-a-component.md) |
| Add a hot-path strategy to an agent | [Add a lane](./add-a-lane.md) |
| Add a new strategist subsystem | [Add an agent](./add-an-agent.md) |
| Add a `.wgsl` shader and a pipeline | [Add a shader](./add-a-shader.md) |
| Decode a new asset format | [Add an asset decoder](./add-an-asset-decoder.md) |
| Project the World into a View for lanes | [Add a flow](./add-a-flow.md) |
| Pin or bound an agent's adaptation | [Control GORNA adaptation](./control-gorna-adaptation.md) |

## Operating and tuning

| I want to… | Recipe |
|---|---|
| Find and fix a problem | [Troubleshoot](./troubleshoot.md) |
| Measure and tune performance | [Profile performance](./profile-performance.md) |
| Investigate one frame or a GORNA decision | [Debug a frame](./debug-a-frame.md) |

---

New here? Start with a [tutorial](../tutorials/your-first-game.md) instead — how-to
guides assume you already know the ropes.
