# Adding behaviour

In [Your first game](./your-first-game.md) you built a static lit scene. Now you
will make the camera *move*: by the end you steer it forward, back, left, and
right with `WASD`, and each step is scaled by the real frame time so movement is
smooth on any machine. You will write the input handling, read the per-frame
delta, and mutate a transform.

Start from the project you finished in the previous lesson.

## Step 1 — Hold some state

`update` runs every frame, but it needs to remember *which* entity to move and
where to read input and time from. Give the game struct fields for those, and
record the camera entity in `setup`.

Replace `struct MyGame;` with:

```rust
use khora_sdk::khora_core::platform::{InputBinding, InputMap};

struct MyGame {
    /// The entity we drive with the keyboard.
    player: Option<khora_sdk::prelude::ecs::EntityId>,
    /// Handle to the engine's input map, cached at setup.
    input_map: Option<Arc<Mutex<InputMap>>>,
    /// Handle to the engine's per-frame time, cached at setup.
    time: Option<khora_sdk::prelude::SharedTime>,
}
```

Update `new`:

```rust
fn new() -> Self {
    MyGame {
        player: None,
        input_map: None,
        time: None,
    }
}
```

The **`InputMap`** maps named actions (like `"player.forward"`) to keys, so your
game logic asks *"is forward pressed?"* instead of testing raw key codes.
**`SharedTime`** is the engine's per-frame clock — it publishes the real frame
delta each tick.

> Why an action map instead of raw keys? It lets players rebind controls and lets
> several keys share one action. See [Input mapping](../how-to/index.md).

## Step 2 — Bind actions and cache handles in `setup`

At the end of your existing `setup`, capture the camera entity and grab the two
resource handles from the **`Runtime`**. The runtime carries every engine service;
`setup` receives it by reference.

Change the camera spawn to store the returned entity, and add the caching block:

```rust
fn setup(&mut self, world: &mut GameWorld, runtime: &Runtime) {
    // ... camera, ground, sun, sphere as in the previous lesson ...

    // Remember the camera as the controllable entity. (Build it with `.build()`
    // and keep the returned EntityId.)
    self.player = Some(
        khora_sdk::Vessel::at(world, Vec3::new(0.0, 2.0, 10.0))
            .with_component(ecs::Camera::new_perspective(
                std::f32::consts::FRAC_PI_4,
                16.0 / 9.0,
                0.1,
                1000.0,
            ))
            .with_rotation(Quaternion::from_axis_angle(Vec3::Y, std::f32::consts::PI))
            .build(),
    );

    // Bind movement actions to keys, then cache the InputMap handle so `update`
    // can query it every frame.
    if let Some(map_arc) = runtime.resources.get::<Arc<Mutex<InputMap>>>() {
        if let Ok(mut map) = map_arc.lock() {
            map.bind("player.forward", InputBinding::Key(KeyCode::KeyW));
            map.bind("player.backward", InputBinding::Key(KeyCode::KeyS));
            map.bind("player.left", InputBinding::Key(KeyCode::KeyA));
            map.bind("player.right", InputBinding::Key(KeyCode::KeyD));
        }
        self.input_map = Some(map_arc.clone());
    }

    // Cache the per-frame Time handle so `update` reads the real frame delta.
    self.time = runtime
        .resources
        .get::<khora_sdk::prelude::SharedTime>()
        .cloned();
}
```

> `update` doesn't receive the `Runtime`, so we cache the handles here. That is
> the standard pattern for any per-frame resource a game needs.

## Step 3 — Read input and the frame delta in `update`

Now the payoff. Each frame: read the real **`delta_seconds`** from `SharedTime`,
ask the `InputMap` which movement keys are held, and nudge the camera transform.
Movement is variable-rate, so it scales by `dt` (not a hardcoded step) — every
machine moves the same distance per second.

Replace `update` with:

```rust
fn update(&mut self, world: &mut GameWorld, _inputs: &[InputEvent]) {
    // Real wall-clock delta from the engine's Time resource. Falls back to a
    // 60 Hz step if the resource isn't available yet.
    let dt = self
        .time
        .as_ref()
        .and_then(|t| t.read().ok().map(|t| t.delta_seconds))
        .unwrap_or(1.0 / 60.0);

    // Resolve movement axes from the action map.
    let (mut forward, mut right) = (0.0_f32, 0.0_f32);
    if let Some(map_arc) = &self.input_map {
        if let Ok(map) = map_arc.lock() {
            if map.is_pressed("player.forward") {
                forward += 1.0;
            }
            if map.is_pressed("player.backward") {
                forward -= 1.0;
            }
            if map.is_pressed("player.left") {
                right -= 1.0;
            }
            if map.is_pressed("player.right") {
                right += 1.0;
            }
        }
    }

    // Move the camera along the world axes, scaled by frame time.
    const SPEED: f32 = 5.0; // units per second
    let velocity = SPEED * dt;
    if let Some(player) = self.player {
        world.update_transform(player, |t| {
            t.translation = t.translation
                + Vec3::Z * (-forward) * velocity
                + Vec3::X * right * velocity;
        });
    }
}
```

**`update_transform`** applies your change *and* syncs the `GlobalTransform` in
one call, so the renderer sees the new pose immediately.

> Why `delta_seconds` and not a fixed step? Gameplay movement is variable-rate;
> only the simulation runs on a fixed timestep. See
> [The frame](../concepts/the-frame.md).

## Step 4 — Run it

```bash
cargo run --release -p my-first-game
```

**You should now see** the same lit scene, but pressing `W` glides the camera
toward the sphere, `S` pulls it back, and `A`/`D` strafe sideways. Movement speed
stays constant whether the game runs at 30 or 300 frames per second.

If nothing moves, confirm you stored the camera in `self.player` (Step 2) and that
the action names in `bind` match the names in `is_pressed` exactly.

## Next steps

You can now spawn a scene and drive it with input. From here:

- [How-to recipes](../how-to/index.md) — full input mapping (including mouse look),
  spawning and transforming entities, parenting.
- Want the full free-fly controller with mouse look? Read
  `examples/sandbox/src/main.rs`, which this lesson is a trimmed slice of.
- Curious why movement uses `delta_seconds` while physics doesn't? Read
  [The frame](../concepts/the-frame.md) — fixed timestep and interpolation.
- Ready to write your own engine subsystem? Continue to
  [Extending the engine](./extending-the-engine.md).
