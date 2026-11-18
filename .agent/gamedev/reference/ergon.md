# Ergon — reference

Domain knowledge for gameplay scripts. Ergon is the game's language: compiled by the engine, run by
the scripting family, hot-reloaded while the game runs. The complete language is the engine book's
`docs/src/reference/ergon.md`; this page is what an agent keeps at hand.

## Scope
Behaviors, the game's own components, systems, events, queries, input actions, spawning, prefabs.

## Where scripts live
- `assets/scripts/**/*.erg` — the only folder compiled. An `.erg` anywhere else is reported, never run.
- Imports are relative to `assets/scripts/`: `import "combat/damage.erg";`. Every module shares one
  flat namespace; a name declared twice is refused.
- `engine/components.erg` is served by the engine (its components); `engine/events.erg` declares
  `Touched(Entity other)` and `Separated(Entity other)`, in scope without an import.
- Scripts run only while the scripting family (`SCRIPT`) is on: in Play in the editor, and in a game
  unless it switches the family off. A **restricted** project never runs them.

## Declarations

| Item | Is |
|---|---|
| `behavior Name { … }` | A component of that name (its fields), with a state companion `Name#state`. Attached by adding the component: in the editor's Inspector, or `GameWorld::add_declared_component(e, "Name", &[…])`. |
| `[Domain(D)] component Name { … }` | A real component the project declares: stored in domain `D` (`Spatial`, `Render`, `Audio`, `Physics`, `Ui`, `Script` — default), saved with scenes, prefabs and game saves. **This is how a game declares components** — never `#[derive(Component)]`. |
| `system Name { … }` | A behavior with no subject, run once a frame on its own entity; works through queries. |
| `event Name(T a, …);` | A declared message: `target.Raise(Name(…))`, handled by `on Name(T a, …) { … }`, delivered next frame. |
| `struct Name { … }` | A value type. |
| `fn T Name(…) { … }` | A free function. |

## Members and lifecycle
- `void Update(float dt)`, `void FixedUpdate(float dt)` (1/60 s fixed clock), `void OnSpawn()`,
  `void OnLoad()` (after a game save restored the instance), `void OnDespawn()`,
  `void OnResumeFailed(string member)`.
- `on Event(…) { … }`, `every 0.5s { … }`, `after 10s { … }`, `async` methods with `await 0.5s;`,
  `state S { … }` with `become S;`.
- `[Critical]`, `[Budget(0.2ms)]`, `[Rate(10hz)]` schedule a behavior or system under budget;
  `[Range(…)]`, `[Tooltip(…)]`, `[Header(…)]` shape a field in the Inspector.

## Talking to the world
- Writes are **queued**, applied at the frame's end: `this.SetPosition(v)`, `e.Translate(d)`,
  `e.SetRotation(q)`, `e.SetScale(s)`, `e.Despawn()`, `e.SetParent(p)`, `e.SetMaterial(m)`.
  `Position()` reads the own entity as the frame began.
- Components: `e.Set(C { field: v })`, `e.Add(C { … })`, `e.Remove(C)`, `e.Get(C)` → `C?`, `e.Has(C)`.
  A script sees its own writes within the frame.
- `Spawn(Vec3 pos, C { … }, …)` → `Entity`; `Spawn(prefab, C { … }, at: p, facing: q)` places a linked
  prefab instance (a `Prefab?` field set in the Inspector).
- Queries: `foreach (Entity e, Health h) with A without B { … }`; `ref C` writes back, in a system only.
- Input: `Pressed("action")`, `JustPressed("action")` — actions, never keys. Actions are bound in Rust
  (`InputMap`, [`../sdk-guide.md`](../sdk-guide.md) §8).
- `Log`, `Warn`, `Error`; maths `Abs`, `Floor`, `Clamp`, `Lerp`, `Sqrt`, …; `Vec3(…)`, `Quat(…)`, `Color(…)`.

## Rules
- A turn that faults applies nothing it queued. Watch the editor's **Problems** tab and the log.
- Editing a declaration while the game runs keeps the data it can: an added field takes its default, a
  renamed one loses its value, a deleted component is orphaned, never deleted.
- Not yet: maps, closures, awaiting events, reading `Transform` with `Get`, playing a `Sound`, loading a
  `Scene`, spawning a `Mesh`. Check the book's "Not yet" before designing around a construct.

## Skills
- [`write-a-script`](../skills/write-a-script/SKILL.md) — a behavior, a component, a system.
- [`setup-input`](../skills/setup-input/SKILL.md) — bind the actions scripts read.
- [`spawn-entity`](../skills/spawn-entity/SKILL.md) — spawn from a script or from Rust.

Verify with Play in the editor: the Console shows `Log` output and the Problems tab what was refused.
