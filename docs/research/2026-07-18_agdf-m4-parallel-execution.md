# AGDF M4 — Parallel Agent Execution: design & first increment

- Date — 2026-07-18
- Status — Complete: parallel execution enabled via a persistent worker pool; critical-path budgeting live (see 2026-07-19 update)
- Scope — `khora-control` scheduler, `khora-core` agent/lane primitives

## Problem

The `ExecutionScheduler` runs a phase's agents strictly sequentially
(`execute_agents_sequential`, `khora-control/src/scheduler.rs`). Each agent
receives an `EngineContext` holding an exclusive `&mut World` **and** an
exclusive `&mut OutputDeck`. AGDF M4 wants agents whose work is independent to
run concurrently, so a frame's cost becomes the critical path rather than the
sum of per-agent costs. The `execute_agents_parallel` stub was an
`unimplemented!` sketch referencing `tokio::spawn` — no async runtime is wired,
and the sketch would have violated two hard invariants.

## Constraints (hard invariants)

1. **RULES §5** forbade `std::thread::spawn` outside `DccService`. Any parallel
   path is a new concurrency site and needs an explicit, named exception.
2. **`LaneContext` is `unsafe impl Send/Sync`** with a `// SAFETY:` invariant
   that assumes a *single-threaded frame scope* (`khora-core/src/lane/mod.rs`).
   Two agents running concurrently must not alias the data those raw pointers
   reach.
3. **`EngineContext` is `!Send`** — it holds `&mut dyn Any` (the world) and
   `&mut OutputDeck`. It cannot cross a thread boundary; each worker must
   construct its own.
4. **The `OutputDeck` is a cross-agent channel** — e.g. ShadowAgent writes
   `ShadowFrame`/`ShadowEntries` that RenderAgent reads back from the *same*
   deck. Sharding the deck breaks this unless shards are merged before the
   consumer runs.

## What is sound (verified)

- `World`, `OutputDeck`, and `LaneBus` are all auto-`Send + Sync` (no raw
  pointers / `Rc` in their fields; component columns are `Box<dyn AnyVec>` with
  `AnyVec: Send + Sync`). A `&LaneBus` can be shared across threads directly.
- `Agent: Send + Sync` already, stored behind `Arc<Mutex<dyn Agent>>`.
- Of the six agents, **four never touch `world` during `execute`** (Shadow,
  Overlay, Ui, Audio — they read the `LaneBus` and write the deck). **Physics
  mutates** the world; **Render reads** it via a `&mut` downcast (camera
  extraction). So Physics/Render are inherently serial; the other four *could*
  run with `world: None`.

## Design (the increment landed here)

An **engine-owned structured executor** — not a tokio pool, not a hand-rolled
persistent pool. Rationale: a persistent pool needs `'static` jobs, but the
frame borrows `&LaneBus`/`&mut World` non-`'static`; the safe idiomatic
primitive that borrows frame data and joins before returning is
`std::thread::scope`. It spawns phase-scoped worker threads that never escape
the frame, satisfying the intent of "engine-owned, spawned under the
concurrency authority" without lifetime-transmuting unsafe or a new dependency.
RULES §5 was amended to name this second exception.

Pieces (all landed, tested):

- **`AgentAccess` + `Agent::access()`** (`khora-core/src/agent/mod.rs`) — a
  defaulted trait method (rule-compliant: agents still implement only
  `Agent`+`Default`). `Exclusive` (default, safe) = needs `&mut World` / touches
  a shared resource → serial. `Isolated` = world-free, writes only its own deck
  → parallel-eligible.
- **`OutputDeck::merge_from`** (`khora-core/src/lane/deck.rs`) — folds a
  worker's private deck shard back into the shared deck; disjoint by the
  eligibility contract, defensive log on `TypeId` collision.
- **`partition_waves`** (`scheduler.rs`, pure + unit-tested) — groups the
  already-topologically-sorted agents into waves: a maximal run of consecutive
  `Isolated` agents with no intra-wave hard-dep; every `Exclusive` agent is a
  singleton. Running waves in order preserves Hard-dependency ordering exactly.
- **`execute_agents_parallel`** (`scheduler.rs`) — singleton waves run inline
  with exclusive `&mut World` (identical to sequential); concurrent waves run
  each agent on a scoped thread with `world: None` + a private deck shard +
  shared `&LaneBus`, folding shards back in wave order (deterministic). Gated by
  `set_parallel_execution(bool)`, **default off**.

With today's all-`Exclusive` declarations every wave is a singleton, so the
parallel path is behaviourally identical to sequential (883→ tests green,
sandbox Vulkan-clean).

## Why live enablement is deferred

Turning the flag on yields no *safe* speedup with the current six agents:

- **Render/Physics** are `Exclusive` (world access) → serial.
- **Overlay/Ui** are world-free but write the shared `Arc<Mutex<FrameGraph>>`;
  concurrent access is memory-safe but makes draw-record ordering
  nondeterministic → not declared `Isolated`.
- **Shadow/Audio** are world-free and deck-only, but live in different phases,
  so no two `Isolated` agents co-occur in a wave.

So the executor is correct infrastructure with no live concurrent wave yet.

## Update — shared-`&World` tier landed (2026-07-18)

Follow-up #1 is done. `EngineContext.world` is now a `WorldAccess<'a>` enum
(`None` / `Shared(&dyn Any)` / `Exclusive(&mut dyn Any)`) reached via
`world_ref()` / `world_mut()`; a new `AgentAccess::SharedWorld` variant grants a
shared `&World`. **RenderAgent** now reads the world through `world_ref()`
(camera extraction is read-only) and declares `SharedWorld` — it no longer
demands an exclusive world borrow. `partition_waves` groups a wave as
Isolated + at most one SharedWorld (a SharedWorld agent may write shared
resources, so two can't share a wave); the concurrent executor hands the lone
SharedWorld agent a shared `&World` (sound: `World: Sync`, proven at compile
time by the scoped-thread capture). Render can therefore now run concurrently
with an `Isolated` agent. Still no *live* concurrent wave in the sandbox: Render
is alone among non-Exclusive agents in OUTPUT (Overlay/Ui remain `Exclusive`
because they write the shared `FrameGraph`).

## Update — FrameGraph isolation + concurrent-executor test landed (2026-07-18)

- **Concurrent executor validated end-to-end**: a scheduler test drives
  `execute_agents_parallel` with a `SharedWorld` reader + an `Isolated` writer
  in one wave and asserts the reader gets a shared `&World`, the writer gets
  none, both run, and their deck shards fold back (`concurrent_exec_tests`).
- **FrameGraph isolation done**: Render/Ui/Overlay no longer lock the shared
  `Mutex<FrameGraph>`. Each buffers its pass into a per-layer deck slot
  (`ScenePassSlot` / `UiPassSlot` / `OverlayPassSlot`, khora-data), and
  `EngineCore::submit_passes` folds them into the graph in the fixed
  scene → ui → overlay order — reproducing the previous insertion order exactly
  (rendering unchanged, verified in the sandbox). Access is now:
  RenderAgent = `SharedWorld` (reads world + writes RenderSystem),
  UiAgent = `SharedWorld` (mutates the shared `UiImageAtlas`),
  OverlayAgent = `Isolated` (own encoder + deck slot, no shared mutable write).
  So `[Ui, Overlay]` is now a valid concurrent wave (Ui the lone SharedWorld,
  Overlay Isolated), with Render its own wave before them.

## Update — parallel execution ENABLED (2026-07-18)

`EngineCore` bootstrap now calls `scheduler.set_parallel_execution(true)`. With
the current agents this runs `[Ui, Overlay]` as a concurrent wave every frame
(Ui the lone `SharedWorld` atlas-writer, Overlay `Isolated`), Render and the
rest serial. Both agents are budgeted ~0.5ms, so overlapping them nets ~0.5ms
against a ~40µs scoped-thread spawn cost — a real win. Validated in the sandbox:
900+ frames, rendering unchanged, no validation errors / panics / deadlocks /
mutex poisoning; 842 tests + clippy green.

## Update — follow-ups completed (2026-07-19)

The four remaining follow-ups are done (the fifth, an end-to-end integration
test, was already landed as `concurrent_wave_runs_shared_reader_and_isolated_writer`).

1. **Persistent worker pool** — `WorkerPool` (`khora-control/src/worker_pool.rs`)
   spawns its threads once in `ExecutionScheduler::new` and joins them on `Drop`.
   The frame's bus is frozen behind `Arc<LaneBus>` after `run_flows`, which makes
   a concurrent wave's `Isolated` agents fully `'static` jobs; the wave's lone
   `SharedWorld` agent now runs **inline** on the calling thread with a shared
   `&World`, so the `World` never crosses a thread boundary — **no `unsafe`, no
   lifetime transmute, no new dependency**. The old per-wave `std::thread::scope`
   is gone. RULES §5 names this as its third exception.

2. **Cost model → critical path** — the scheduler publishes its wave grouping
   each frame via `TelemetryEvent::WavePlan`; the DCC stores the latest plan and
   `GornaArbitrator::set_wave_plan` hands it to `fit_budgets`, which now costs
   each wave by its `max` member (critical path) and spends the budget against
   the sum over waves. `forecast_total_ms` does the same. **Safety property:**
   with an all-singleton grouping (serial execution, or an empty plan) sum-of-
   wave-maxes ≡ sum-of-costs, so the serial path is bit-identical to before.

3. **Per-agent deck-write declarations** — `Agent::deck_writes()` (defaulted,
   rule-compliant) declares the deck slot `TypeId`s an agent writes;
   `check_wave_deck_disjoint` verifies co-wave agents write disjoint slots at
   wave formation, naming the offending agents. `OutputDeck::merge_from` keeps
   its defensive collision log as a backstop.

4. **Broader validation** — the live `[Ui, Overlay]` wave was exercised in the
   sandbox and editor (rich UI + gizmos): rendering unchanged, deterministic, no
   Vulkan validation errors / panics / deadlocks / mutex poisoning.

### Still open (not in this pass)
- Costing sub-stepped fixed agents by their sub-step multiplicity (orthogonal to
  concurrency; the fit still counts each agent once per phase membership).
- Growing the live concurrent surface beyond `[Ui, Overlay]` (more `Isolated`
  agents per phase) — infrastructure now supports it; no agent qualifies yet.
