# AGDF M4 — Parallel Agent Execution: design & first increment

- Date — 2026-07-18
- Status — Foundation implemented (executor off by default); live enablement deferred
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

## Remaining path (follow-ups)

1. **Shared-`World` read tier (H3)** — add a `world_shared: Option<&World>`
   access mode so Render's camera extraction can run without an exclusive
   borrow, making Render parallel-eligible against a world-free agent.
2. **FrameGraph isolation** — give Overlay/Ui their own deck-shard draw lists
   merged deterministically, so they can be `Isolated`.
3. **Per-agent deck-write declarations** — replace the defensive
   collision-log in `merge_from` with a compile-of-schedule check that two
   `Isolated` agents in a wave never write the same slot type.
4. **Cost model → critical path** — GORNA `fit_budgets` still sums per-agent
   costs (correct for serial). Once waves run concurrently, the fit must budget
   the wave's *max* cost, not its sum (the executor already records per-agent
   wave timings).
5. **Integration test** — drive `execute_agents_parallel` with two synthetic
   `Isolated` mock agents writing disjoint deck slots; assert both outputs land
   and ordering is deterministic. (Wave partitioning + deck merge are unit
   tested now; the end-to-end scoped-thread path is not.)
