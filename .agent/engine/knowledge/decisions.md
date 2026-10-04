# Knowledge — Architecture Decisions

Durable decisions and their rationale. Append when a new structural decision lands.

1. **Single-acquire frame lifecycle** — `begin_frame()` acquires the swapchain texture once; all
   agents encode to that same target; `end_frame()` presents. Prevents Vulkan semaphore errors and
   enables correct `LoadOp::Load` compositing.

2. **Lane is the universal pipeline** — every hot-path execution (render, physics, audio, asset, ECS)
   goes through `Lane::execute()`. Agents orchestrate lanes but never do pipeline work themselves.

3. **Typed data-flow via LaneBus / OutputDeck** — lanes read typed Views from the `LaneBus` and write
   typed outputs into the `OutputDeck`. No global mutable state; `Flow::adapt` was removed (Flows are
   read-only projectors).

4. **CRPECS for ECS** — archetype-based, SoA storage, components `'static + Send + Sync`, queries are
   the primary data access pattern. AGDF re-tiles columns (SoA↔AoSoA) as self-maintenance, never
   changing semantics.

5. **Abstract GPU resource IDs** — all wgpu resources behind typed IDs (`TextureId`, `BufferId`, …).
   Raw handles never leak into public APIs.

6. **Agent vs Service** — only subsystems that negotiate a GORNA budget are Agents (Render, Shadow,
   Overlay, Physics, Ui, Audio). Non-negotiating work uses direct services (`AssetService`,
   `SerializationService`, `EcsMaintenance`).

7. **SDK is a façade** — the Scheduler, `BudgetChannel`, and `EnginePlugin` are internal; game code
   only sees the `khora-sdk` public surface.

8. **Substrate Pass owns tick ordering** — the Scheduler runs DataSystem invariants and Flow
   projection before the agent descent; `khora-control` depends on `khora-data` only to *invoke* the
   substrate, not to drive layout.

9. **4 bind groups, always** — render lanes use exactly 4 bind groups (Frame / Object / Material /
   Lighting); new lighting features add bindings to group 3, never a 5th group (wgpu universal baseline).

10. **Shaders as files** — WGSL lives in `shaders/pipelines/` + `shaders/lib/`, composed via
    the `PipelineSystem` backend + `naga_oil` `#import`; no inline source, no runtime filesystem reads.

11. **Documentation in place, no ADR files** — record decisions in existing docs, not a separate ADR
    section. (Project convention.)

12. **Stable asset identity via a lazy registry** — an asset's `AssetUUID` must not derive from its path
    (rename breaks it) nor its content (edit breaks it), so identity is a persisted token. It is stored in a
    single project file `<root>/.khora/asset-registry.ron` (RON, one `(uuid, path)` per line, **sorted by UUID**
    for line-oriented git merges, written atomically), *not* per-asset sidecars (chosen to avoid doubling the
    file count) and *not* a monolithic blob (chosen to avoid a merge/corruption hotspot). **Lazy freeze**: no
    entry ⇒ the `new_v5(path)` default (back-compatible with every existing project/test); the first rename/move
    freezes the current UUID. Scenes/prefabs/`.kmat` store an `AssetUUID` as the serde newtype `khora.AssetUUID`
    (a reserved name the record codec recognises as an asset reference), so a frozen UUID means moving a file
    breaks no reference, with no remap. The read side is engine infrastructure (`IndexBuilder`,
    `PackBuilder`, runtime all resolve through it ⇒ dev/release parity); the editor (`ProjectVfs`) is the sole
    writer. The file sits outside `assets/`, so it is never scanned, watched, or packed.
- **Type-keyed maps stay separate (2026-09-27).** `AnyMap` (frame blackboard, `Arc` values), `LaneBus`
    (Views, `Box`), `OutputDeck` (lane outputs, `Box`), `LaneContext` (per-execution, not `Send`) and
    `TypedRegistry` (runtime resources/services/backends) are each a `HashMap<TypeId, …>`. They differ in
    ownership (`Arc` vs `Box`) and thread bounds, and `LaneContext` is on the per-frame hot path; a shared
    generic would save ~20 lines each and touch every lane. Revisit only if a sixth appears.

## Persistence redesign (2026-10)

13. **Records by name, entities by `PersistentId`** — a save holds the WHAT: components by registered name,
    fields by name, entities by a 64-bit identity independent of where they sit (`EntityId` is recycled).
    Authored ids are random (no branch collisions), created ids numbered, prefab members derived by
    `within(root, inner)`. Memory layout never reaches a file, so AGDF can re-tile freely. Renames go
    through `formerly`, removals through `RetiredComponent`; an unknown name fails the load — a removal is
    a decision, and the decision is written down. No payload migrations.

14. **Snapshot is a cache, never the only copy** — `FastestLoad` writes positional bytes bound to every
    component's schema fingerprint and is refused whole on any mismatch: 9x faster to load, readable only by
    the build that wrote it. Used for same-build round trips (editor Play/Stop). A component the tracer
    cannot guard falls back to compact.

15. **Atomic loads** — read, check and stage everything (reserving ids) before adding anything; commit
    page by page or abandon. A failed load never half-populates a world; every adaptation is reported in a
    `LoadReport` instead of being silently absorbed.

16. **Authored scene vs observed save** — a scene holds what an author or a tool wrote (provenance
    `Authored`/`ToolAuthored`); a game save holds only how play differed from its base scene, with each
    changed component's value at save time (`before`). Loading is a three-way merge onto the scene *as it
    is now*, so a level fix reaches existing saves wherever play left the value alone. Engine-written state
    a save must keep opts in with `resumable`.

17. **Prefab links as a delta** — an instance is saved as `InstanceRecord { root, prefab, delta }`, the
    delta being the same `SaveRecord` shape as a game save; load expands the prefab as it is now and merges
    the delta. A prefab edit reaches every instance field that was not overridden. Placement (root
    translation/rotation) and the link itself are never overrides.

18. **Scripts run only while Playing** — an editor editing a scene is not playing it. Enforced by mode
    registration (`register_agent_for_mode`) and the `ScriptFlow` gate, not by each script. The mode is a
    runtime resource the app writes and the scheduler reads once per frame.

19. **Tiered resume for suspended script bodies** — one path for next frame, hot reload and game load:
    exact → unchanged → rebuilt at the same named site → restarted with the original arguments →
    abandoned with `OnResumeFailed`. Sites and fingerprints are named from source structure, never from
    indices, so an unrelated edit does not throw away a running body.

20. **Old format readers are retired after a one-time upgrade** — `xtask assets upgrade-scenes` rewrote
    v1 scenes once, then the readers and the tool were deleted (`2daeeb9`). Older files are refused with
    `SceneFileReadError::OldFormat`; the engine carries one format, not a museum.
