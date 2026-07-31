# The frame

How Khora turns the [CLAD](./clad.md) layering into motion — the per-frame
descent, the Substrate Pass, and the fixed-timestep model that keeps the
simulation deterministic.

---

## Two clocks, one channel

Khora has two clocks. The **hot path** runs every frame on the main thread, at
60 Hz or higher, and must never block. The **cold path** runs at ~20 Hz on a
background thread, watches what just happened, and decides what should happen
next. They communicate through a single channel: budgets flow from the cold path
to the hot path; telemetry flows back.

```mermaid
sequenceDiagram
    participant OS as OS / winit
    participant SDK as EngineCore
    participant App as EngineApp
    participant GW as GameWorld
    participant RS as RenderSystem
    participant Sch as Scheduler
    participant FG as FrameGraph
    participant DCC as DCC (~20 Hz thread)

    OS->>SDK: redraw requested
    SDK->>SDK: drain_inputs()
    SDK->>SDK: run_app_update (Pre/Post-Sim + Pre-Extract DataSystems around app.update)
    SDK->>App: app.update(world, inputs)
    SDK->>RS: begin_frame() → ColorTarget, DepthTarget
    SDK->>Sch: run_frame()
    Sch->>Sch: budget_channel.sync()
    Sch->>Sch: run Flows → publish Views into LaneBus
    Sch->>Sch: per phase: plugins, topo sort, execute agents
    Note over Sch: Agents record GPU passes into FrameGraph; lanes fill the OutputDeck
    SDK->>FG: drain + submit (topological pass order)
    SDK->>RS: end_frame(presents) → swapchain present
    SDK->>SDK: run_maintenance (drain OutputDeck, EcsMaintenance compaction)
    DCC-->>Sch: budgets via BudgetChannel
```

The cold path is the subject of [The big idea](./saa.md) and the GORNA
reference. This page is about the hot path — the six stages a frame walks
through, and the timing model underneath them.

## Startup, once

Before the first frame, the engine boots through the SDK entry point. It opens a
window, runs your bootstrap closure (which typically registers the renderer),
constructs your app, registers the default services, the DCC, and the agents,
then calls `setup` once so you can spawn initial entities and cache service
handles. Finally the DCC walks every registered agent and lets each cache its
services exactly once. Nothing in `setup` is ever re-run; after it, the engine
enters the frame loop and stays there.

The details of bootstrapping an app are a how-to, not a concept — see the
[SDK quickstart how-to](../how-to/spawn-and-transform.md) for the actual steps.

## The six-stage descent

Each frame runs six stages in order. Every stage is a public method on the
engine core, so drivers (the editor's overlay and shell) can interleave hooks
between them:

```
1. drain_inputs        ← Pop queued InputEvents, tick telemetry
2. run_app_update      ← Substrate invariants (Pre/Post-Sim, Pre-Extract) around app.update
3. begin_render_frame  ← RenderSystem::begin_frame, swapchain acquire
4. run_scheduler       ← Substrate Pass (Flows publish Views) + phase-by-phase agent execution
5. end_render_frame    ← submit FrameGraph + RenderSystem::end_frame
6. run_maintenance     ← Maintenance DataSystems drain the OutputDeck; EcsMaintenance compacts
```

A few stages reward a closer look, because they are where the architecture's
discipline shows.

**Stage 2 — `run_app_update`** runs the data layer's invariants *around* your
game logic: pre-simulation systems (input-driven mutations the app will see),
then `app.update`, then post-simulation systems (hierarchy fix-ups such as
transform propagation), then pre-extract systems (GPU mesh sync). Notice what is
*not* here: scene projection. That happens in Stage 4.

**Stage 4 — `run_scheduler`** is the heart of the descent. It runs in two parts.
First the **Substrate Pass** runs every registered projection
[**Flow**](../reference/glossary.md), publishing each domain's typed View into
the [**`LaneBus`**](../reference/glossary.md) for lanes to consume. Then the
Scheduler runs every active execution phase in order — `INIT`, `OBSERVE`,
`TRANSFORM`, `MUTATE`, `OUTPUT`, `FINALIZE` — syncing budgets from the DCC,
running plugin hooks, topologically sorting the phase's agents by their hard
dependencies, and executing them. This is the `Control → Agent → Lane → Data`
descent made concrete: the Scheduler is Control, it dispatches agents, agents
invoke lanes, lanes read the Views the Substrate just published.

**Stage 6 — `run_maintenance`** is where the data layer does its own
housekeeping. The maintenance systems drain the
[**`OutputDeck`**](../reference/glossary.md) the lanes filled — audio and physics
write-backs land on ECS components here — and the ECS maintenance pass compacts
storage pages and prunes orphaned data. This pass is *Data-owned and
self-budgeted*: it is not negotiated with the DCC. It is the home of the data
layer's self-optimization, the same "Data adapts itself" principle from
[CLAD](./clad.md).

> **Two output channels.** Lanes feed two sinks. The `FrameGraph` carries
> recorded GPU render passes, drained and submitted in Stage 5. The typed
> `OutputDeck` carries cross-domain results (audio, physics, …), drained by
> maintenance systems in Stage 6. The *inputs* to lanes are the typed Views the
> projection Flows publish into the `LaneBus` in Stage 4.

The six stages are the single most important sequence in Khora. Everything
performance-critical happens here, in this order. To watch a real frame walk
through them, see [Debug a frame](../how-to/debug-a-frame.md).

## The Substrate Pass

The **Substrate** is the data layer's per-tick self-presentation: the invariants
that keep the World consistent (Stage 2) and the projection Flows that publish
read-only Views for the lanes (Stage 4). The Scheduler *invokes* the Substrate
because it owns the tick ordering, but it does not decide *how* the data is laid
out — that remains the data layer's own concern.

The crucial property: Flows are **read-only projectors**. A Flow reads the World
and publishes a typed View; it never mutates game state and never changes which
components an entity has. This is the frame-level expression of *adapt the HOW,
never the WHAT*. Lanes consume those Views from the bus rather than querying the
World directly — which is what lets the data layer change its internal
representation freely without any lane noticing.

## Fixed-timestep simulation and render interpolation

Rendering runs at the display's variable rate, but the **simulation** must
advance in fixed increments to stay frame-rate independent and deterministic. A
physics step that depended on a variable frame delta would produce different
results on a 30 Hz machine and a 144 Hz machine — unacceptable for physics,
replays, or networked play. Khora reconciles the two with a single time
**accumulator**, owned by the Scheduler.

The model, per frame:

1. **Measure and clamp.** Take the real wall-clock delta since the previous
   frame and clamp it to `MAX_FRAME_DELTA_SECONDS` (0.25 s). A longer real gap —
   a debugger break, an asset hitch, a window drag — is truncated so the
   accumulator never demands an unbounded catch-up. This is the classic
   *spiral-of-death* guard.
2. **Accumulate.** Add the clamped delta to the accumulator.
3. **Consume whole steps.** With the fixed step `fixed_delta` (the smallest
   `fixed_timestep` any agent declares — in practice the physics agent's, default
   1/60 s), compute `steps = floor(accumulator / fixed_delta)`, capped at
   `MAX_SIM_STEPS` (5). The remainder carries over to the next frame.
4. **Derive alpha.** The leftover fraction — `remainder / fixed_delta`, always in
   `[0, 1)` — becomes the render `interpolation_alpha`.
5. **Run.** Step the fixed-timestep agents exactly `steps` times (a fixed-update
   sub-loop, each iteration a full agent invocation), then run the regular phase
   loop **once**, excluding those agents so they are not stepped twice. So
   physics integrates *N* discrete sub-steps while the render fires exactly once.

```mermaid
sequenceDiagram
    participant Loop as Frame
    participant Acc as sim_accumulator
    participant Sim as Fixed agents (physics)
    participant Render as Render (once/frame)

    Loop->>Acc: += min(real_dt, 0.25 s)
    Note over Acc: steps = floor(acc / fixed_delta), capped at 5
    loop steps times
        Acc->>Sim: step(fixed_delta)
    end
    Acc->>Loop: alpha = remainder / fixed_delta  (in [0, 1))
    Loop->>Render: render once, blend by alpha
```

The step arithmetic is small and pure — its essence is:

```rust
let acc = accumulator + dt;            // dt already clamped to 0.25 s
let steps = (acc / fixed_delta).floor();    // whole sim steps this frame
// ... capped at MAX_SIM_STEPS, the excess dropped so the accumulator stays bounded
let alpha = (remainder / fixed_delta).clamp(0.0, 1.0);   // render blend factor
```

**The determinism guarantee** is the payoff: the *same total elapsed time
produces the same number of sim steps regardless of frame cadence*. A 144 Hz
burst and a 30 Hz stutter that span the same wall-clock interval run identical
step counts. Game logic that must be deterministic therefore never sees a
variable step. Variable-rate game code instead reads the real delta; render
smoothing reads the interpolation alpha.

On saturation — when more than `MAX_SIM_STEPS` would be required — the excess
time is dropped rather than queued. Under sustained overload the simulation runs
in *slow motion* rather than freezing in an ever-deepening catch-up, and the
accumulator stays bounded. This is a deliberate trade: a brief slowdown is
recoverable; a spiral of death is not.

All of this timing state lives in one place — the engine's
[**`Time`**](../reference/glossary.md) resource (`khora_core::time::Time`),
republished fresh into the runtime each frame before the render phase reads it. It
carries the real `delta_seconds`, the `fixed_delta_seconds`, the
`interpolation_alpha`, and a monotonic `frame` counter.

### Why interpolation is render-only

Because the simulation steps at a fixed cadence but the screen refreshes at a
different one, the most recent simulated pose rarely lands exactly on a frame
boundary. Rendering the raw current pose would judder. Instead the render path
blends the *previous* and *current* poses by `interpolation_alpha`, producing
smooth motion at any refresh rate.

Here is the architecturally important part: the previous-pose store for
interpolation is an engine-internal **resource**
(`khora_core::interpolation::TransformInterpolation`), **not** an ECS component.
A post-simulation system snapshots each simulated body's world transform into
that store before propagation overwrites it; the render path reads it to blend.

This is *adapt the HOW, never the WHAT* in its purest form. Interpolation is a
render representation: it carries no game meaning, so it must never appear as a
component in the editor inspector or in a saved scene. Only simulated entities
(those carrying a rigid body) are snapshotted; everything else simply renders at
its current transform. The store is pruned each pass so a despawned body leaves
no stale entry. Keeping it out of the ECS is what guarantees the smoothing can
never leak into simulation semantics — a saved scene is identical whether or not
the renderer ever interpolated it.

## Next steps

- **[Debug a frame](../how-to/debug-a-frame.md)** — walk a live frame through the
  six stages and inspect a GORNA decision.
- **[The big idea](./saa.md)** — the cold path that feeds budgets into Stage 4,
  and the philosophy the frame serves.
- **[Glossary](../reference/glossary.md)** — every proprietary term in one place.
