# Map input to actions

This guide shows you how to bind keys and mouse buttons to named actions, read them each
frame, and drive movement with the real frame delta — the same path the sandbox's player
controller uses.

> **Prerequisites.** You have an `EngineApp` with `setup(&mut self, world, runtime)` and
> `update(&mut self, world, inputs)` ([Your first game](../tutorials/your-first-game.md)).

## Bind actions in `setup`

The engine owns one `InputMap`, registered as a resource behind an `Arc<Mutex<…>>`. In
`setup`, lock it and `bind` each action to one or more bindings — an action fires when
**any** of its bindings fires (logical OR), so you can map both WASD and the arrow keys to
the same action. Cache the handle so `update` can query it.

```rust
use std::sync::{Arc, Mutex};
use khora_sdk::khora_core::platform::{InputBinding, InputMap};
use khora_sdk::prelude::KeyCode;

const ACTION_FORWARD: &str = "player.forward";
const ACTION_JUMP: &str = "player.jump";

fn setup(&mut self, world: &mut GameWorld, runtime: &Runtime) {
    if let Some(map_arc) = runtime.resources.get::<Arc<Mutex<InputMap>>>() {
        if let Ok(mut map) = map_arc.lock() {
            map.bind(ACTION_FORWARD, InputBinding::Key(KeyCode::KeyW));
            map.bind(ACTION_FORWARD, InputBinding::Key(KeyCode::ArrowUp));
            map.bind(ACTION_JUMP, InputBinding::Key(KeyCode::Space));
        }
        self.input_map = Some(map_arc.clone()); // Option<Arc<Mutex<InputMap>>> field
    }
}
```

`InputBinding` is `Key(KeyCode)` or `Mouse(MouseButton)`. The engine folds the frame's
input into the map once per frame, before your `update` runs (the `input_map_update`
system, first in `PreSimulation`).

The map and the `inputs` slice handed to `update` read the same stream, the runtime's
`Channel<InputEvent>`. Neither takes the events from it: each reads under a cursor of
its own, so both see every event, whichever runs first. The stream keeps the last 2048
events (`INPUT_BACKLOG`) for a reader that has stopped reading — a minimised window, a
system that ran once — and drops the oldest past that.

## Read actions in `update`

Lock the cached handle and query it. The map distinguishes **held** from **edge**:

```rust
if let Some(map_arc) = &self.input_map {
    if let Ok(map) = map_arc.lock() {
        if map.is_pressed(ACTION_FORWARD) {
            // held this frame — continuous movement
        }
        if map.just_pressed(ACTION_JUMP) {
            // fired only on the press edge — one-shot jump
        }
        // `map.just_released(action)` fires on the release edge.
    }
}
```

`is_pressed` is true every frame the action is held; `just_pressed` / `just_released` are
true only on the transition frame and clear on the next update.

**Losing focus lets go of everything.** When the window stops receiving input (an
alt-tab), the OS never reports the release of a key held then. The platform sends
`InputEvent::FocusLost` instead, and the map releases every held action, reporting each
as `just_released` that frame: a character sprinting when the player tabbed away stops,
and does not sprint on when the window comes back. Code that derives its own state from
the raw `inputs` must do the same on `FocusLost`.

## Use the real frame delta

For variable-rate movement, multiply by the wall-clock delta, not a constant. Cache the
`SharedTime` handle in `setup` and read `delta_seconds` in `update`:

```rust
// setup:
self.time = runtime.resources.get::<khora_sdk::prelude::SharedTime>().cloned();

// update:
let dt = self
    .time
    .as_ref()
    .and_then(|t| t.read().ok().map(|t| t.delta_seconds))
    .unwrap_or(1.0 / 60.0); // fall back to a 60 Hz step if unavailable
```

## Move an entity from input

Combine the three pieces — read the action, scale by `dt`, write the transform:

```rust
use khora_sdk::prelude::math::Vec3;

let speed = 5.0;
if let Some(player) = self.player {
    world.update_transform(player, |t| {
        if held_forward {
            t.translation = t.translation + Vec3::new(0.0, 0.0, -speed * dt);
        }
    });
}
```

> **Mouse motion** is not an action — the `InputMap` models boolean inputs only. For
> look/drag, match `InputEvent::MouseMoved { x, y }` on the raw `inputs` slice passed to
> `update`, as the sandbox's controller does.

## Expected result

Holding `W` (or `Up`) moves the player smoothly at a frame-rate-independent speed;
pressing `Space` triggers exactly one jump per press. Unbound keys do nothing.

## Related

- [Spawn entities and move them](./spawn-and-transform.md) — the transform you drive.
- [Play 3D audio](./play-3d-audio.md) — trigger a sound on `just_pressed`.
- [`InputMap` and `Time` reference](../reference/sdk.md) — bindings, edges, timing fields.
