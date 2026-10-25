# Coming from another engine

<p class="kp-lede">If you have made games in Unity, Godot or Unreal, most of Khora will feel
familiar: a scene of things, a component list per thing, an inspector, a play
button. This page maps what you know onto Khora's words, then names the two
ideas that are genuinely new.</p>

## The same ideas, Khora's words

| You know | In Khora | Read |
|---|---|---|
| GameObject / Node / Actor | an **entity** — an id, nothing more; what it is comes from its components | [Data and the ECS](../concepts/ecs.md) |
| Component (Unity), node types (Godot) | a **component**, stored with every other entity's of the same kind | [Add a component](../how-to/add-a-component.md) |
| `Transform` | `Transform` — position, rotation, scale; parents through the Scene Tree | [Spawn and transform](../how-to/spawn-and-transform.md) |
| MonoBehaviour / GDScript / Blueprint | an Ergon **behavior**, attached as a component | [Scripting](../concepts/scripting/index.md) |
| a "manager" script looping over objects | an Ergon **system** with a query | [Behaviors and systems](../concepts/scripting/behaviors-and-systems.md) |
| Scene / Level | a **scene**: what you authored, saved as records by name | [Scenes and game saves](../concepts/saves.md) |
| Prefab / PackedScene / Blueprint class | a **prefab**: a linked instance with per-field overrides | [Prefabs](../concepts/prefabs.md) |
| Play mode | **Play**: the scene is snapshotted, scripts run, **Stop** puts it back | [Editor reference](../reference/editor.md) |
| `Resources` / `res://` / content browser | the **Assets** panel over `<project>/assets/`, every asset known by a stable id | [Assets](../concepts/assets.md) |
| PlayerPrefs / save systems you write | **game saves**, built in: what the game changed, merged onto the scene on load | [Scenes and game saves](../concepts/saves.md) |

## What is genuinely new

**The engine adapts while the game runs.** In most engines, quality settings are
chosen up front and stay put. In Khora, every big subsystem — rendering,
physics, audio, scripts — can do its job several ways, at several costs, and the
engine renegotiates every frame how much time each one gets, watching the frame
time, the heat, the battery. A laptop that starts to throttle gets cheaper
shadows, not a stutter. You do not write this; it is the engine's. [The big
idea](../concepts/saa.md) explains it from the start.

**A script asks; it never acts at once.** `this.SetPosition(p)` does not move
the entity on that line: it queues the move, and the engine applies every
script's requests together at the end of the frame. Reading back what you just
wrote in the same frame is the one habit to unlearn — [Changing the
world](../concepts/scripting/writing.md) shows what you get in exchange: scripts
that run beside the renderer, a script that can be stopped part-way and resumed,
and a script that fails leaving nothing half-done.

## Where next

- In the editor straight away: [Scripting](../concepts/scripting/index.md) has
  you write a first behavior in five steps.
- From Rust, without the editor: [Your first game](./your-first-game.md).
