# Khora SDK — Guide

The Rust side of a game: the public `khora-sdk` surface, with examples written against it. Gameplay
itself is Ergon ([`reference/ergon.md`](./reference/ergon.md)). The reference game is the engine's
`examples/sandbox/`; the full surface is the engine book's `docs/src/reference/sdk.md`.

- Document — Khora SDK Guide v2.0
- Status — Active

---

## 1 — When a game has Rust

A project starts data-only: scenes, assets and Ergon scripts, run by the prebuilt `khora-runtime`
(which calls `khora_sdk::run_default()`). The hub's **Add Native Code** adds `Cargo.toml` and a
`src/main.rs` calling `run_default()`; replace its body with an `EngineApp` when the game needs to:

- bind input actions (`InputMap`) for its scripts' `Pressed` / `JustPressed`;
- install its own runtime resources (`provide`), switch families (`Activation`), save and load games;
- register different backends, or a new subsystem (a family — [`skills/add-a-family`](./skills/add-a-family/SKILL.md)).

`khora-sdk` is the one dependency. What it does not name at its root it re-exports as crates:
`khora_sdk::khora_core`, `khora_sdk::khora_data`, `khora_sdk::khora_lanes`, `khora_sdk::inventory`.

## 2 — A minimal native game

```rust
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use khora_sdk::prelude::ecs::Camera;
use khora_sdk::prelude::math::Vec3;
use khora_sdk::prelude::*;
use khora_sdk::winit_adapters::WinitWindowProvider;
use khora_sdk::{
    open_game_assets, run_winit, EngineApp, GameWorld, HostMonitors, PhaseProvider,
    PipelineSystem, RenderSystem, Runtime, Vessel, WgpuPipelineSystem, WgpuRenderSystem,
    WindowConfig,
};

#[global_allocator]
static GLOBAL: SaaTrackingAllocator = SaaTrackingAllocator::new(std::alloc::System);

struct MyGame;

impl EngineApp for MyGame {
    fn window_config() -> WindowConfig {
        WindowConfig { title: "My Game".to_owned(), ..WindowConfig::default() }
    }

    fn new() -> Self {
        MyGame
    }

    fn setup(&mut self, world: &mut GameWorld, _runtime: &Runtime) {
        let camera = Camera::new_perspective(std::f32::consts::FRAC_PI_4, 0.1, 1000.0);
        Vessel::at(world, Vec3::new(0.0, 2.0, 10.0)).with_component(camera).build();
    }

    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
}

// A required super-trait of `EngineApp`; its defaults add and remove no phase.
impl PhaseProvider for MyGame {}

/// The folder holding the game: its pack once built, the project's `assets/` in a dev run.
fn game_dir() -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    match exe_dir {
        Some(dir) if dir.join("data.pack").is_file() => dir,
        _ => PathBuf::from(env!("CARGO_MANIFEST_DIR")),
    }
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    run_winit::<WinitWindowProvider, MyGame>(|window, runtime, _event_loop| {
        let mut rs = WgpuRenderSystem::new();
        // No frame runs without a renderer: run_winit panics on its failure as well.
        let monitors = rs.init(window).expect("renderer init failed");
        runtime.resources.insert(HostMonitors(monitors));
        // The device is a backend of its own: render lanes reach it apart from the system.
        runtime.backends.insert(rs.graphics_device());
        let rs: Box<dyn RenderSystem> = Box::new(rs);
        runtime.backends.insert(Arc::new(Mutex::new(rs)));

        // Required: every render lane resolves its pipeline through it.
        match WgpuPipelineSystem::new() {
            Ok(sys) => {
                let sys: Arc<dyn PipelineSystem> = Arc::new(sys);
                runtime.resources.insert(sys);
            }
            Err(e) => log::error!("pipeline system init failed: {e}"),
        }

        if let Err(e) = open_game_assets::<MyGame>(runtime, &game_dir()) {
            log::error!("the game's assets did not open: {e:#}");
        }
    })
}
```

`prelude::*` brings the `ecs`, `materials` and `math` **modules**, not their contents: name
`ecs::Camera`, or import `prelude::ecs::*`.

## 3 — The run, in order

`run_winit` creates the window (hidden), calls `new`, runs the bootstrap closure, checks the seal (§6),
boots the engine, paints the first frame, shows the window, and loops until it closes.

| Hook | When |
|---|---|
| `window_config()` | Before the window is created; the default `game_identity` reads its title. |
| `game_identity()` | At boot: the game the engine files what it learns under. Default: the title at `ENGINE_VERSION`. |
| `keeps_user_files()` | At boot: whether the engine keeps a profile and a quarantine in the user's folders. Default `false`. |
| `new()` | Once, before the bootstrap closure — no engine context. |
| bootstrap closure | Register backends and open the game's assets into `&mut Runtime`. |
| `setup(world, &Runtime)` | Once, after engine init — spawn, bind input, cache handles. |
| `provide(&self, &mut Runtime)` | Once, after `setup`, before the families are built — insert the resources a family reads. |
| `update(world, inputs)` | Every frame — Rust game logic. |
| `startup_ready()` | Once, on the first frame that finds startup over (every `Readiness` ticket landed, or 60 s passed). Drop a loading screen here. |
| `on_shutdown()` | Once, on exit. |

`before_frame`, `before_agents`, `after_agents` and `intercept_window_event` exist for a tool's UI
overlay (the editor's egui). A family switched on or off in `before_frame` holds for that whole frame.

## 4 — Backends (in the bootstrap closure)

The renderer and the pipeline system are required (§2). Register the rest only if the game uses them:

```rust
use khora_sdk::{
    AudioDevice, AudioMixBus, AudioStream, CpalAudioDevice, DefaultMixBus, LayoutSystem,
    PhysicsProvider, RapierPhysicsWorld, StreamInfo, TaffyLayoutSystem,
};

let physics: Box<dyn PhysicsProvider> = Box::new(RapierPhysicsWorld::default());
runtime.backends.insert(Arc::new(Mutex::new(physics)));

let layout: Box<dyn LayoutSystem> = Box::new(TaffyLayoutSystem::new());
runtime.backends.insert(Arc::new(Mutex::new(layout)));

// The mix bus is where the audio lanes meet the device's real-time thread.
let mix_bus: Arc<dyn AudioMixBus> =
    Arc::new(DefaultMixBus::new(StreamInfo { channels: 2, sample_rate: 48_000 }, 8192));
runtime.resources.insert(Arc::clone(&mix_bus));
let device: Box<dyn AudioDevice> = Box::new(CpalAudioDevice::new());
match device.open(mix_bus) {
    // Kept as a backend: dropping the stream stops the sound.
    Ok(stream) => runtime.backends.insert(Arc::<dyn AudioStream>::from(stream)),
    Err(e) => log::error!("audio open failed: {e}"),
}
```

Text needs nothing: the boot installs the engine's shaper and rasterizer.

## 5 — Assets and scripts

`open_game_assets::<A>(runtime, dir)` opens, beside `dir`:

- a **pack** (`data.pack`, `index.bin`, `manifest.bin`) — its seal verified with the key stamped in the
  binary; its scripts compiled from the pack only;
- **loose `assets/`** — only in a binary with no key; indexed, and its `assets/scripts/` mounted.

It inserts `Arc<Mutex<AssetService>>` in `runtime.services` and fails when the seal refuses the pack,
the index is unreadable, or there is neither a pack nor `assets/`. The boot then files, off the frame,
the pack's scripts to compile — or the loose files to index and their scripts to compile — and startup
waits on each through `Readiness` (`startup_ready` fires once they landed). `run_default()` does all of it.

Without `open_game_assets`, `khora_sdk::scripts::mount(runtime, &assets_root)` mounts a folder's
scripts alone; one mounted in the bootstrap closure is compiled before `setup`, so `setup` can attach
its behaviors (the sandbox does this).

## 6 — The seal at every start

Every start checks the binary against what lies beside it, whatever the entry point:

| Beside the binary | Binary with no key (dev run) | Stamped binary (built game) |
|---|---|---|
| no pack | starts | refused |
| a pack | refused (`PackSealError::Unstamped`) | starts only if `manifest.bin` verifies |

`run_winit` panics on a refusal; `run_default` returns the error before a window opens. A stamped
binary never reads loose files.

## 7 — The world

| Do | Call |
|---|---|
| Spawn | `Vessel::at(world, pos)…build()`, `spawn_plane(world, size, y)`, `spawn_cube_at(world, pos, size)`, `spawn_sphere(world, radius, segments, rings)`, `world.spawn((components…))` |
| Move | `world.update_transform(e, \|t\| …)` (syncs `GlobalTransform`), or `get_transform_mut` then `sync_global_transform(e)` |
| Read / write | `get_component::<T>(e)`, `get_component_mut::<T>(e)`, `add_component`, `remove_component`, `query::<Q>()`, `query_mut::<Q>()` |
| Attach a script's component | `add_declared_component(e, "Hover", &[("speed", ScriptValue::Float(0.9))])` → `Result<(), DeclaredComponentError>` |
| Hierarchy | `set_parent(child, Some(parent))` |
| Materials | `add_material(StandardMaterial { … })` → `MaterialRef`; a mesh with no material is not drawn |
| Despawn | `despawn(e)` |

`add_declared_component` takes a component an Ergon script declares (a `behavior` or a `component`);
named fields take the values given, the others the script's defaults. A Rust component goes through
`add_component`.

## 8 — Input

The engine owns one `Arc<Mutex<InputMap>>` in `runtime.resources`, updated before `update`. Bind actions
in `setup`; Rust reads them through the cached handle, Ergon through `Pressed("action")` /
`JustPressed("action")`. Actions are bound only here — a project with no Rust binds none.

```rust
use khora_sdk::khora_core::platform::{InputBinding, InputMap};

// In `setup`; `input_map: Option<Arc<Mutex<InputMap>>>` is a field of the game.
if let Some(map) = runtime.resources.get::<Arc<Mutex<InputMap>>>() {
    if let Ok(mut map) = map.lock() {
        map.bind("player.jump", InputBinding::Key(KeyCode::Space));
        map.bind("player.jump", InputBinding::Mouse(MouseButton::Left)); // bindings OR together
    }
    self.input_map = Some(Arc::clone(map));
}
```

`is_pressed` (held), `just_pressed` / `just_released` (edges). Mouse motion and wheel are raw
`InputEvent::MouseMoved { x, y }` / `MouseWheelScrolled { delta_x, delta_y }` in `update`'s `inputs`.
The frame delta: cache `runtime.resources.get::<SharedTime>()` in `setup`, read `delta_seconds`.

## 9 — Families: Activation and scopes

Every family (rendering, shadows, physics, audio, UI, scripting, loading, overlays, the game's own)
starts on. `Arc<Activation>` is in `runtime.resources` before `setup`:

```rust
use khora_sdk::Activation;
use khora_sdk::khora_core::physics::PHYSICS;

if let Some(activation) = runtime.resources.get::<Arc<Activation>>() {
    if let Err(e) = activation.deactivate(PHYSICS) {
        log::warn!("{e}");
    }
}
```

- `activate(id)`, `deactivate(id)`, `scope(id, "Marker")` are **requests**, settled once per frame
  after `before_frame`; `scope` runs the family only on entities holding the component named `Marker`.
- An unknown id is refused with `UnknownFamily`. Ids: `khora_core::script::SCRIPT`,
  `khora_core::physics::PHYSICS`, `khora_core::audio::AUDIO`, a family's `LaneFamily::AGENT`.
- Ergon runs only while `SCRIPT` is on. To switch from `update`, cache the `Arc<Activation>` in `setup`.

## 10 — Saves and scenes

A **game save** is how the running world differs from the scene it started from:

```rust
use std::path::Path;

use khora_sdk::khora_core::asset::AssetUUID;
use khora_sdk::khora_core::scene::LoadTicket;
use khora_sdk::{fs, GameWorld, SceneFile, SerializationGoal};

const MAX_SAVE_BYTES: u64 = 16 * 1024 * 1024;

/// Writes how `world` differs from `level` (known as `level_id`) to `path`.
fn save(world: &GameWorld, level_id: AssetUUID, level: &SceneFile, path: &Path) -> anyhow::Result<()> {
    let save = world.save_game(level_id, level, SerializationGoal::SmallestFileSize)?;
    fs::write_atomic(path, &save.to_bytes())?;
    Ok(())
}

/// Files the save at `path` to load while frames run; its outcome lands under the ticket.
fn file_load(world: &mut GameWorld, level: &SceneFile, path: &Path) -> anyhow::Result<LoadTicket> {
    let bytes = fs::read_bounded(path, MAX_SAVE_BYTES)?;
    let save = SceneFile::from_bytes(&bytes).map_err(|e| anyhow::anyhow!("not a save: {e:?}"))?;
    Ok(world.file_load_game(&save, level))
}

/// Logs the load filed as `ticket` once it landed; returns whether it did.
fn landed(world: &mut GameWorld, ticket: LoadTicket) -> bool {
    match world.take_load_outcome(ticket) {
        None => false,
        Some(Ok(report)) => {
            report.entries.iter().for_each(|entry| log::info!("restore: {entry}"));
            true
        }
        Some(Err(e)) => {
            log::error!("the save did not load: {e}");
            true
        }
    }
}
```

Call `landed` from `update` each frame while a ticket is held.

- Every load is **atomic**: an error leaves the world as it was; a `LoadReport` lists what was
  adapted. `load_game` is the synchronous form, for `setup`.
- The save is merged onto the level **as it is now**; keep the level a save names
  (`SerializationService::new().save_base(&save)`). `FastestLoad` is written compact for a save.
- The engine names no save folder; the game picks one.
- A **scene** (`.kscene`) is the whole world: `SerializationService::new().save_world(world.inner_world(), goal)`
  and `replace_world` / `load_world`. A scene linking prefabs needs `SerializationService::with_prefabs(source)`.
  Detail: [`skills/load-scene`](./skills/load-scene/SKILL.md).

## 11 — Re-exports worth knowing

| Need | Path |
|---|---|
| Startup wait | `Readiness`, `ReadinessHold` (`runtime.resources`, `Arc<Readiness>`) |
| Scene loads in flight | `SceneLoadHub`, `LoadOutcome` |
| Signed packs | `PackKey`, `PackSigner`, `SignedManifest`, `PackSealError`, `stamp_pack_key` |
| Trust | `khora_sdk::trust` (`ProjectTrust`, refusals, quarantine) |
| Environment map | `EnvironmentMap::from_asset(uuid)` (insert in `provide`) |
| Constants | `ENGINE_VERSION`, `DEFAULT_SCENE_REL_PATH`, `PRIMARY_VIEWPORT` |

---

*When in doubt, read `examples/sandbox/src/` and the engine book's `docs/src/reference/sdk.md`.*
