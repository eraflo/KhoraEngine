# Serialization

Three strategies, one service, one file format. How Khora saves and loads scenes.

- Document — Khora Serialization v1.0
- Status — Authoritative
- Date — May 2026

---

## Contents

1. Three strategies, three goals
2. SerializationGoal
3. The .kscene file format
4. SerializationService
5. Component serialization
6. Play mode snapshots
7. For game developers
8. For engine contributors
9. Decisions
10. Open questions

---

## 01 — Three strategies, three goals

A scene file has more than one consumer. The editor wants something readable. Release builds want something tiny. Play mode wants something instant. Khora serializes through whichever strategy fits the goal.

| Strategy | Format | Lane | Use case |
|---|---|---|---|
| **Definition** | RON (human-readable) | `DefinitionSerializationLane` | Debug, long-term storage, scene authoring |
| **Recipe** | Binary commands | `RecipeSerializationLane` | Compact, editor interchange |
| **Archetype** | Binary layout | `ArchetypeSerializationLane` | Fastest load, play-mode snapshot |

The strategy is selected by `SerializationGoal`, not by file extension. The `.kscene` header records which strategy produced the payload, so loading is symmetric.

## 02 — SerializationGoal

```rust
pub enum SerializationGoal {
    HumanReadableDebug,   // → Definition
    LongTermStability,    // → Definition
    EditorInterchange,    // → Recipe
    Performance,          // → Archetype
    FastestLoad,          // → Archetype (alias)
}
```

The mapping from goal to strategy lives in `SerializationService::pick_strategy`. Choosing a goal is a developer decision; choosing a strategy is an engine decision.

## 03 — The .kscene file format

```
.kscene file
┌─────────────────────────────────────┐
│ Header (64 bytes)                   │
│  Magic: "KHORASCN" (8 bytes)        │
│  Version: 1 (4 bytes)               │
│  Strategy ID (32 bytes)             │
│  Payload length (8 bytes)           │
│  Reserved (12 bytes)                │
├─────────────────────────────────────┤
│ Payload (bincode or RON encoded)    │
└─────────────────────────────────────┘
```

The header is fixed-size — 64 bytes — so the loader can parse it without any prior format knowledge. The strategy ID tells the loader which lane to dispatch.

A `SceneFile` in memory is `SceneHeader + SerializedPage[]`. Pages map directly to ECS archetype pages, which is how Archetype-strategy load can be near-`memcpy` fast.

## 04 — SerializationService

```rust
let service = ctx.services.get::<Arc<SerializationService>>().unwrap();

// Save
let scene_file = service.save_world(&world, SerializationGoal::FastestLoad)?;
std::fs::write("scene.kscene", scene_file.to_bytes())?;

// Load
let bytes = std::fs::read("scene.kscene")?;
let file = SceneFile::from_bytes(&bytes)?;
service.load_world(&file, &mut world)?;
```

The service owns the three strategy lanes. It picks the right one based on the requested goal (for save) or the header strategy ID (for load). Scene I/O is on-demand — there is no "serialization agent" because there are no per-frame strategies to negotiate. See the *Agent vs Service* rule in [Architecture](./02_architecture.md).

## 05 — Component serialization

Every component derived with `#[derive(Component)]` gets:

- A `SerializableT` mirror struct with `Encode` / `Decode`.
- `From<T>` for `SerializableT` and the reverse.
- An `inventory::submit!` registration for scene serialization.

The mirror exists because GPU handles, runtime caches, and trait objects do not serialize. Fields marked `#[component(skip)]` are excluded from the mirror — they are reconstructed on load (typically by the asset system or the agent's `on_initialize`).

For components that need a fully manual mirror (unit structs, components holding `Box<dyn Trait>`), `#[component(no_serializable)]` skips the auto-generation and you write `Serialize` / `Deserialize` by hand.

The registration is the seam: scene loading walks the inventory, instantiates the right `SerializableT`, decodes it, converts to `T`, attaches to the entity. No string lookups, no dynamic dispatch in the hot path.

## 06 — Play mode snapshots

Play mode uses Archetype strategy for fast snapshot/restore:

```rust
// Press Play:
let service = SerializationService::new();
let scene_file = service.save_world(&world, SerializationGoal::FastestLoad)?;
world_snapshot = Some(scene_file.to_bytes());

// Press Stop:
let scene_file = SceneFile::from_bytes(&snapshot)?;
service.load_world(&scene_file, &mut world)?;
```

The snapshot/restore is fast because Archetype strategy serializes pages directly, with minimal transformation. A 10 000-entity scene snapshots and restores in milliseconds.

> **Physics state is not preserved.** When restoring, the physics engine rebuilds from component data. Velocities and contacts are reset to defaults. A "physics snapshot" goal is on the [Open questions](./open_questions.md).

## 07 — For game developers

```rust
// Save the current scene
let service = services.get::<Arc<SerializationService>>().unwrap();
let scene = service.save_world(&world, SerializationGoal::HumanReadableDebug)?;
std::fs::write("my_scene.kscene", scene.to_bytes())?;

// Load a scene at startup
fn setup(&mut self, world: &mut GameWorld, services: &ServiceRegistry) {
    let service = services.get::<Arc<SerializationService>>().unwrap();
    let bytes = std::fs::read("levels/level_01.kscene").unwrap();
    let scene = SceneFile::from_bytes(&bytes).unwrap();
    service.load_world(&scene, world).unwrap();
}
```

For your own components, derive `Component`. If a field should not be serialized (a GPU handle, a runtime accumulator), mark it `#[component(skip)]`. Provide a `Default` so the field can be reconstructed.

Editor scene files use the Definition strategy — they are RON, hand-editable in a pinch. Release scenes typically use Archetype for load speed.

## For engine contributors

The split:

| File | Purpose |
|---|---|
| `crates/khora-core/src/scene/` | `SceneFile`, `SceneHeader`, `SerializationGoal`, `SerializationStrategy` trait |
| `crates/khora-io/src/serialization/` | `SerializationService`, strategy registration |
| `crates/khora-lanes/src/scene_lane/` | `DefinitionSerializationLane`, `RecipeSerializationLane`, `ArchetypeSerializationLane` |
| `crates/khora-data/src/ecs/components/registrations.rs` | Component inventory |
| `crates/khora-macros/src/lib.rs` | `#[derive(Component)]` — generates `SerializableT` |

Adding a fourth strategy (e.g., DeltaSerialization for save games and undo/redo, on the roadmap): create a new lane implementing `SerializationStrategy`, register it in `SerializationService`, add a `SerializationGoal` variant that maps to it, write tests covering save → load round-trip.

The hardest part is not the strategy; it is verifying the round-trip is lossless across all 25+ standard components. Existing tests cover this; new strategies must add their own.

## Decisions

### We said yes to
- **Three strategies, one file format.** A single `.kscene` magic, three payload encodings. The header carries the strategy ID.
- **`#[derive(Component)]` generates the mirror.** Two structs to maintain by hand was the single biggest source of serialization bugs.
- **Play mode uses Archetype.** Snapshot speed is load-bearing for editor responsiveness.
- **Editor uses Definition.** Hand-editable scenes are useful in CI, in code review, in ten-year-old Git histories.

### We said no to
- **Reflection-based serialization.** Considered. Rejected. The proc macro is faster, statically checked, no allocation.
- **A "serialization agent."** No strategies to negotiate per-frame. Scene I/O is a service.
- **Preserving physics state across play mode.** Considered. Rejected for v1 — physics rebuilds from components, which is consistent and predictable. May change.

## Open questions

1. **DeltaSerialization.** Roadmap item. Save games and undo/redo both want incremental snapshots. The trait surface is sketched, not implemented.
2. **Physics snapshot goal.** Should there be a `SerializationGoal::IncludePhysicsState` that captures velocities, sleep state, contacts?
3. **Versioned components.** Today, scene format version is tracked in the header. Component schema versions are not. A scene saved against an older component definition may fail to load.

---

*Next: how the engine watches itself. See [Telemetry](./15_telemetry.md).*
