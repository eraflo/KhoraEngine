# Knowledge — Architecture Decisions

Durable decisions and their rationale. Append when a new structural decision lands.

1. **Single-acquire frame lifecycle** — `begin_frame()` acquires the swapchain texture once; all
   agents encode to that same target; `end_frame()` presents. Prevents Vulkan semaphore errors and
   enables correct `LoadOp::Load` compositing.

2. **Lane is the universal pipeline** — every hot-path execution (render, physics, audio, script, UI)
   goes through a `khora_core::lane::Lane`, which records at one of its levels (`Lane::record`). Agents
   choose lanes but never do pipeline work themselves. *(Superseded wording: it first named
   `Lane::execute()`, and asset/ECS lanes, which became services; the old lane API went in 3k, 2026-11.)*

3. **Typed data-flow via LaneBus / OutputDeck** — lanes read typed Views from the `LaneBus` and write
   typed outputs into the `OutputDeck`. No global mutable state; `Flow::adapt` was removed (Flows are
   read-only projectors).

4. **CRPECS for ECS** — archetype-based, SoA storage, components `'static + Send + Sync`, queries are
   the primary data access pattern. AGDF re-tiles columns (SoA↔AoSoA) as self-maintenance, never
   changing semantics.

5. **Abstract GPU resource IDs** — all wgpu resources behind typed IDs (`TextureId`, `BufferId`, …).
   Raw handles never leak into public APIs.

6. **Agent vs Service** — only subsystems that negotiate a GORNA budget are agents, each a **family**
   run by the one `LaneAgent` (renderer, shadows, grid, wireframe, gizmo, skybox, physics, ui, audio,
   script, loading). Non-negotiating work uses direct services (`AssetService`, `SerializationService`,
   `EcsMaintenance`).

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
    *(2026-11: `LaneContext` is gone (3k1) — lanes take typed inputs and outputs — so four maps remain.)*

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

18. **Scripts run only while their family is on** — an editor editing a scene is not playing it. The
    engine has no mode: Play and Stop are the editor's, which switches the scripting family
    (`khora_core::script::SCRIPT`, with physics and audio) off while editing through the `Activation`
    resource. Enforced by the activation and by roles — the script DataSystems and `ScriptFlow` are
    `Role::Serves(SCRIPT)` — not by each script. Requests settle once per frame at the frame boundary, so
    a whole frame sees one state; mail sent to scripts while they are off is dropped (`Mailbox`, `Drop`).

19. **Tiered resume for suspended script bodies** — one path for next frame, hot reload and game load:
    exact → unchanged → rebuilt at the same named site → restarted with the original arguments →
    abandoned with `OnResumeFailed`. Sites and fingerprints are named from source structure, never from
    indices, so an unrelated edit does not throw away a running body.

20. **Old format readers are retired after a one-time upgrade** — `xtask assets upgrade-scenes` rewrote
    v1 scenes once, then the readers and the tool were deleted (`2daeeb9`). Older files are refused with
    `SceneFileReadError::OldFormat`; the engine carries one format, not a museum.

21. **A behavior is a component** — `behavior Guard` declares the component `Guard` (authored, its
    fields) and its state companion `Guard#state` (`Runtime` + `resumable`); adding `Guard` to an entity is
    attaching the behavior. Behavior names are global (two modules declaring one is a compile error). The
    legacy `Script`/`ScriptState` components were converted once by a one-time xtask upgrade, then retired
    (`RetiredComponent`) — the same shape as decision 20. An arriving instance takes its fields from its row;
    the game save's merge, not the lane, carries a scene edit to a field the game left alone.

## Agents/lanes contract (2026-11)

22. **One kind of agent, one protocol** (3k, commits `f5b3658c`…`80df266f`) — a subsystem is a family:
    typed `Lane`s, a `LaneFamily`, one `register_agent!` registration; its agent is always `LaneAgent`, the
    only `Agent` implementor outside tests (xtask `only_lane_agent_implements_agent`). The `Agent` trait is
    `offer` → `apply` → `report` (GORNA), `place` (required), `wake`, `execution_timing`, `contention`; no
    `execute` — the scheduler runs each family's installed layer and wakes the agent only for an event. No
    agent reaches the `World`, so waves split on `Contention` alone. The app installs runtime entries in
    `EngineApp::provide` and registers no agent (`AgentProvider` removed). Rationale: one code path for every
    subsystem, nothing a hand-written agent could do that a family cannot, and contention the only thing to
    declare. A fixed-step family's step lives at its place (`Place::fixed_step`).
