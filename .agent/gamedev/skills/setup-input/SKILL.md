---
name: setup-input
description: Wires player input in a Khora game — actions bound on the engine's InputMap in Rust, read by Ergon scripts (Pressed / JustPressed) or by Rust, plus raw InputEvents for mouse motion. Use when adding controls.
---

# Set up input

Two layers: the **`InputMap`** of named actions (held / just pressed / just released), and raw
**`InputEvent`s** for what the map does not model (mouse motion, wheel). Scripts name actions, never
keys; actions are bound only from Rust, so a game whose scripts read input needs native code.

## Steps
1. Name actions once, as `const`s (`const JUMP: &str = "player.jump";`), and use the same strings in
   scripts.
2. In `setup`, bind them on the engine's map and cache the handle (`update` has no `runtime`):
   ```rust
   use std::sync::{Arc, Mutex};
   use khora_sdk::khora_core::platform::{InputBinding, InputMap};
   use khora_sdk::prelude::{KeyCode, MouseButton};

   if let Some(map) = runtime.resources.get::<Arc<Mutex<InputMap>>>() {
       if let Ok(mut map) = map.lock() {
           map.bind(JUMP, InputBinding::Key(KeyCode::Space));
           map.bind(JUMP, InputBinding::Mouse(MouseButton::Left)); // bindings OR together
       }
       self.input_map = Some(Arc::clone(map));
   }
   ```
3. Read them:
   - in Ergon: `if (JustPressed("player.jump")) { … }`, `if (Pressed("player.forward")) { … }`;
   - in Rust `update`: lock the cached map, `is_pressed`, `just_pressed`, `just_released`.
4. Raw events in Rust `update`:
   ```rust
   for event in inputs {
       if let InputEvent::MouseMoved { x, y } = event { /* look */ }
   }
   ```
5. Scale movement by the frame delta: cache `runtime.resources.get::<SharedTime>().cloned()` in `setup`,
   read `delta_seconds` in `update` (a script's `Update(float dt)` gets it directly).

## Rules
- Lock the `InputMap` briefly; never hold it across heavy work.
- `KeyCode` variants follow the W3C UI Events codes (`KeyW`, `Space`, `ArrowUp`, `ShiftLeft`).

See `PlayerController` in the engine's `examples/sandbox/src/player.rs`. Verify: Play / `cargo run` and
press the keys.
