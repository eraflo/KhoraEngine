---
name: write-a-script
description: Writes gameplay in Ergon — a behavior, a component the game declares, a system or an event — under assets/scripts/. Use for any gameplay rule, game state on entities, or reaction to collisions and input.
---

# Write a script

Gameplay is Ergon. Reference: [`../../reference/ergon.md`](../../reference/ergon.md); the full language is
the engine book's `docs/src/reference/ergon.md`.

## Steps
1. Create `assets/scripts/<name>.erg` (subfolders allowed; nothing outside `assets/scripts/` compiles).
2. Declare the game's data as components, the rule as a behavior or a system:
   ```text
   [Domain(Physics)]
   component Health { int current = 100; int max = 100; }

   event Damaged(int amount);

   behavior Guard {
       float speed = 2.0;

       void OnSpawn() { this.Add(Health { }); }

       void Update(float dt) {
           if (Pressed("guard.advance")) { this.Translate(Vec3(0.0, 0.0, -speed * dt)); }
       }

       on Damaged(int amount) {
           if (var h = this.Get(Health)) {
               this.Set(Health { current: h.current - amount });
               if (h.current - amount <= 0) { this.Despawn(); }
           }
       }
   }
   ```
3. Attach the behavior: in the editor, select an entity → Inspector → add the `Guard` component; or from
   Rust, `world.add_declared_component(e, "Guard", &[("speed", ScriptValue::Float(3.0))])`.
4. Bind any action the script reads (`"guard.advance"`) in Rust — [`../setup-input/SKILL.md`](../setup-input/SKILL.md).
5. Play in the editor. Read the Console and the Problems tab; fix what the compiler refused.

## Rules
- Components are declared in Ergon, never with `#[derive(Component)]` in a game.
- A world write is queued: `Position()` sees a `SetPosition` next frame.
- An event is declared before it is raised or handled; a handler takes all its parameters or none.
- `ref` query bindings write back in a `system` only; a behavior writes others with `e.Set(C { … })`.
- Check the book's "Not yet" list before relying on a construct; a refused one is a compile error, not a
  runtime surprise.
