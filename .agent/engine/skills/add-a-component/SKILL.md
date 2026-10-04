---
name: add-a-component
description: Defines and registers a new ECS component in the data layer. Use when adding gameplay or engine state that entities carry, with serialization handled by the derive macro.
---

# Add a component

Components live in `crates/khora-data/src/ecs/components/`. The `#[derive(Component)]` macro generates the
`SerializableX` mirror, `From` conversions, and `inventory` self-registration.

## Steps
1. Define the struct in the right `components/<domain>/` file:
   ```rust
   #[derive(Component)]
   #[component(domain = Render)]            // semantic domain; provenance defaults to Authored
   pub struct MyThing {
       pub value: f32,
       #[component(skip)]                    // runtime-only, not serialized
       pub gpu: Option<TextureId>,
   }
   ```
2. **Choose a provenance** (`ComponentProvenance`): `Authored` (default — saved, offered in
   "+ Add Component"), `ToolAuthored` (saved, set by a tool action, e.g. `Parent`, `PrefabInstance`),
   `Derived` (recomputed from authored state — never saved), `Runtime` (per-run state — never saved).
   For `Runtime` state a **game save** must keep so play resumes, add `resumable`
   (`#[component(domain = Script, provenance = Runtime, resumable)]`, like `ScriptState`).
3. Field attributes: `#[component(skip)]` for GPU handles / runtime caches. Do **not** reach for
   `#[component(no_serializable)]`: it removes the `ComponentRegistration` outright — the component is then
   never saved, never inspected, never found by name (`AudioListener` and `ActiveEvents` were silently
   dropped from saved scenes this way).
4. The derive self-registers via `inventory` — there is no registration list to edit. Only generics
   (`HandleComponent<T>`) and hand-written impls stay explicit in `World::new`.
5. **Snapshots:** `FastestLoad` needs the mirror's serde format traced in full (`schema_complete()`); a
   field type the tracer cannot follow makes the component unguarded and the save falls back to compact.
   Check with a snapshot round-trip (see `khora-data/tests/snapshot_unguarded.rs`).
6. Add `#[cfg(test)]` round-trip tests through `SerializationService` (record and snapshot).

## Changing a shipped component later
- **Rename** the type or a field: keep the old name in `#[component(formerly = "Old")]` (repeatable; on a
  field it becomes a serde alias). Loads report `Renamed`.
- **Add a field:** it takes its default on load (`Defaulted`). **Remove one:** its value is dropped
  (`Dropped`). No migration code.
- **Remove the type:** declare it retired, or every save naming it fails to load:
  `inventory::submit!{ khora_data::scene::RetiredComponent { name: "Old" } }` (loads report `Retired`).
- Never change what an existing field *means* under the same name — records are matched by name.

## Rules
- Components are `'static + Send + Sync`. Don't store raw asset data inline — use `AssetHandle<T>` /
  `HandleComponent<T>` / `MeshRef` / `MaterialRef`.

## Verify
`cargo gate`. For storage/layout details, consult [`../../reference/ecs-data.md`](../../reference/ecs-data.md).
