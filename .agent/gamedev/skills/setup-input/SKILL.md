---
name: setup-input
description: Wires player input in a Khora game using the InputMap action system and raw InputEvents. Use when adding controls — movement, actions, mouse look.
---

# Set up input

Two layers: the **`InputMap`** for boolean actions (held / pressed / released), and raw **`InputEvent`s**
for things the map can't model (mouse motion, scroll).

## Steps
1. Centralize action names as `const` strings (`const ACTION_FORWARD: &str = "player.forward";`).
2. In `setup`, get `Arc<Mutex<InputMap>>` from `runtime.resources`, then bind:
   ```rust
   map.bind(ACTION_FORWARD, InputBinding::Key(KeyCode::KeyW));
   map.bind(ACTION_FORWARD, InputBinding::Key(KeyCode::ArrowUp)); // multiple bindings OR together
   ```
   Cache the `Arc<Mutex<InputMap>>` on your game struct — `update` has no `runtime` argument.
3. In `update`:
   ```rust
   if map.is_pressed(ACTION_FORWARD) { /* move */ }
   for ev in inputs { if let InputEvent::MouseMoved { x, y } = ev { /* look */ } }
   ```

## Rules
- Lock the `InputMap` mutex briefly; don't hold it across heavy work.
- `KeyCode` / `MouseButton` / `InputBinding` come from the SDK (`prelude` / `khora_sdk`).

See `examples/sandbox/src/main.rs` `PlayerController` for the full pattern. Verify with `cargo run`.
