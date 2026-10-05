# ECS / Data — reference

Domain knowledge for Khora's data layer. Consulted during Research (dispatch the `codebase-*` subagents
to apply it to concrete files). Follow [`../RULES.md`](../RULES.md) §3 (HOW-not-WHAT), §6 (persistence)
and §7 (components).

## Scope
CRPECS internals, component storage and queries, the AGDF layout learner, component registration, the
read-only Flow / DataSystem machinery — and **persistence**: scene records, identities, encodings, the
snapshot, atomic loads, game saves and prefab links.

## Key files
- ECS: `crates/khora-data/src/ecs/` (`world/`, `storage.rs`, `soa.rs`, `query/`, `query_plan.rs`, `bitset.rs`).
- AGDF layout: `crates/khora-data/src/ecs/layout/` (`LayoutAdvisor` in `mod.rs`, `Ucb1` in `bandit.rs`, `DecayCounter` in `decay.rs`).
- Components: `crates/khora-data/src/ecs/components/` (self-registered through `inventory`).
- Provenance: `ComponentProvenance` in `crates/khora-data/src/ecs/component_registry.rs`.
- Registration: `ComponentRegistration` (`provenance`, `formerly`, `resumable`, `column_to_record`, `stage`,
  `schema`/`schema_complete`, snapshot fns, inspector `to_json`/`from_json`) in
  `crates/khora-data/src/scene/component_registration.rs`.
- Flows / DataSystems: `crates/khora-data/src/flow/`, `crates/khora-data/src/ecs/systems/`.
- Derive macro: `crates/khora-macros/src/component.rs`.
- Persistence: `crates/khora-data/src/scene/` — `record/` (`Record`, `LoadReport` in `report.rs`),
  `scene_record.rs` (`SceneRecord`, `PageRecord`, `InstanceRecord`), `capture.rs`, `apply.rs`
  (`prepare`/`Prepared`/`Identity`), `encoding/`, `snapshot/`, `save.rs` (`SaveRecord`), `prefab/`,
  `file.rs` (`SceneFileReadError`), `retired.rs`.
- Identity: `crates/khora-core/src/ecs/persistent_id.rs` (`PersistentId`).
- Service: `crates/khora-io/src/serialization.rs` (`SerializationService`, `AssetPrefabs`).

## Hard rules
- `#[derive(Component)]` for every component (auto `SerializableX` + `From` + `inventory` registration).
  Attributes and their effect: [`../conventions.md`](../conventions.md) §4. `no_serializable` removes the
  registration (never saved/inspected) — it is not "runtime state"; use a provenance.
- **AGDF adapts representation only** (SoA↔AoSoA), self-bounded by a cost/benefit test, observed by the DCC
  via telemetry — it never changes which components an entity has.
- **Flows are read-only projectors** (`select → project`); they never mutate the World. World mutation goes
  through `DataSystem` invariants registered via `inventory` — never wired by hand.
- The `Ucb1` bandit is deterministic (no RNG) for reproducibility.
- **Queries:** `World::query(&self)` requires `ReadOnlyWorldQuery` (an `unsafe` marker; a new query term that
  only reads must implement it, one that writes must not). Writing goes through `query_mut`, which panics on a
  query naming one component twice (`WorldQuery::accessed_type_ids`, undeduplicated). A `&mut T` item is reached
  through `vec.as_mut_ptr().add(row)` (`query/mod.rs::column_item`), never `get_mut`/`get_unchecked_mut` on the
  column — a slice reborrow invalidates items already handed out (Miri, Stacked Borrows).
- **Spawning:** `spawn` panics on an unregistered component type or a bundle naming one twice; `try_spawn`
  returns `SpawnError`. Registering a component clears the query-plan cache.
- Verify `unsafe` query/storage changes under Miri: `cargo +nightly miri test -p khora-data --lib -- <filter>`.

## Persistence

**Records.** A world is captured page by page (`capture_world` / `capture_subtree`) as a `SceneRecord`:
components by registered name, fields by name, entities by `PersistentId`. Memory layout never reaches a
file. Provenance decides what a scene keeps: `Authored` + `ToolAuthored` (`ComponentRegistration::is_saved`).
A game save additionally keeps `resumable` components.

**`PersistentId`** — 64 bits, top bit = namespace:
- *authored* — random 63 bits (`random_authored`), so two branches adding entities do not collide;
- *created* — `created(n)`, numbered by the world for entities the game or code spawned;
- *within an instance* — `within(root, inner)`: UUIDv5 over both, under the fixed `WITHIN` namespace.
  Nested instances compose. Never change `WITHIN`.

**Goal → encoding** (`SerializationService::encoding_for`, header id in the file):

| `SerializationGoal` | Encoding | Id |
|---|---|---|
| `HumanReadableDebug`, `LongTermStability` | pretty JSON (`TextEncoding`) | `KH_TEXT_V2` |
| `EditorInterchange`, `SmallestFileSize` | compact binary, names once (`CompactEncoding`) | `KH_COMPACT_V2` |
| `PortableBinary` | MessagePack with names (`MsgPackEncoding`) | `KH_MSGPACK_V2` |
| `FastestLoad` | positional snapshot (`snapshot/`) | `KH_SNAPSHOT_V1` |

**Snapshot.** Positional, no names; every component listed with its schema fingerprint (`schema()`, the
traced serde format of its mirror). Any fingerprint mismatch refuses the file whole. A component whose
`schema_complete()` is false cannot be snapshotted: `save_world` warns and writes `KH_COMPACT_V2` instead.
A game save asking for `FastestLoad` is written compact. The snapshot is a same-build cache (editor
Play/Stop, `khora-editor/src/scene_io/mod.rs`) — never a project's only copy.

**Format version.** `SCENE_FORMAT_VERSION` is 2 (`khora-core/src/scene/format.rs`). Older files fail with
`SceneFileReadError::OldFormat` — the v1 readers were removed after a one-time upgrade (commit `2daeeb9`).
`NotAScene` / `NotASave` guard the two magic headers.

**Atomic load.** `prepare` checks every component name, reserves an id per entity and stages every value;
only then does `Prepared::commit(world, Identity::Keep | Fresh)` add, page by page (no migration, no
orphan rows). `Prepared::abandon` returns the reserved ids. Unknown name → whole load fails, unless the
name is `formerly` of a type (reported `Renamed`) or declared retired (reported `Retired`). The
`LoadReport` lists every adaptation (`ReportKind`); callers should show it.
`SerializationService::{load_world, replace_world}` — `replace_world` stages before despawning anything.

**Game saves** (`save.rs`). A scene holds what was authored; a `SaveRecord` holds, against its `base`
scene (`AssetUUID`), what play changed: `changes` + `before` (the scene's value when saved), `created`,
`destroyed`, removed components, hierarchy `order`. Load (`compose_reporting` → `prepare_game`) is a
**three-way merge** with the scene *as it is now*: only fields the game changed overwrite today's value,
so a later scene edit reaches every value the game left alone. A scene's removal wins
(`RemovedFromScene`). `promote` folds a save's observed values back into a scene as authored.

**Prefab links** (`prefab/`). An instance root carries `PrefabInstance { prefab: AssetUUID }`
(`ToolAuthored`); members are keyed `within(root, inner)`. A record keeps an instance as an
`InstanceRecord { root, prefab, delta: SaveRecord }` — `collapse` on save, `expand` on load (the prefab as
it is now, then the delta on top). A linked scene needs a `PrefabSource` (`AssetPrefabs` reads them
through `AssetService`); without one, `NoPrefabs` makes the load fail, and saves are written expanded. A
missing prefab or a prefab containing itself refuses the load; an unreadable prefab at save time is saved
expanded with its link kept. A `.kprefab` is a compact scene file of the subtree (`serialize_subtree`,
`serialize_prefab` — never a link to itself). Spawn with `instantiate_prefab` (linked) or
`instantiate_subtree`.

**Inspector overrides** (apply / revert). Data side: `prefab/overrides.rs` (`instance_of`, `prefab_world`,
`apply_to_prefab`, `PrefabApply`). Editor side: `khora-editor/src/ops/prefab_overrides/mod.rs`
(`json_overrides`, `reverted`, `revert_instance`) and `commands/prefab.rs`. The root's `translation` /
`rotation` and the `PrefabInstance` link are never overrides. UX: [`editor-ui-ux.md`](./editor-ui-ux.md).

## Traps
- A component with `provenance = Derived | Runtime` found in a scene is skipped (`NotSaved`), not an error.
- `formerly` on a type is matched only after an exact name miss (`registration_named`).
- `capture_save` and `load_game` need the base scene's bytes: a save is meaningless without its scene.

## Skills
- [`add-a-component`](../skills/add-a-component/SKILL.md) — define + register a component.
- [`debug-frame`](../skills/debug-frame/SKILL.md) — investigate a missing/stale `View` or layout thrash.
- [`build-and-test`](../skills/build-and-test/SKILL.md) — verify (round-trip tests in `khora-data/tests/scene_records/`).
