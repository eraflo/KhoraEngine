# Profile and tune performance

**Goal:** measure where a frame spends its time, read why GORNA is making the choices it makes, and
pull the levers that actually move the needle. This page is task-oriented and honest about what is
measurable today and what is not yet — for the *why* behind each mechanism, follow the links out.

1. [Read frame performance](#read-frame-performance)
2. [Understand GORNA's decisions](#understand-gornas-decisions)
3. [The fixed-timestep cost model](#the-fixed-timestep-cost-model)
4. [AGDF / data layout](#agdf--data-layout)
5. [Flow view caching](#flow-view-caching)
6. [Practical tips](#practical-tips)
7. [What is not yet measurable](#what-is-not-yet-measurable)

---

## Read frame performance

Telemetry is a first-class subsystem, not an afterthought — it is the engine's nervous system and
the input to every adaptive decision. See [Telemetry](../concepts/telemetry.md).

**What the engine measures.**

- **The frame.** The host reports every frame (`FrameReport`): how long it kept each device busy,
  the waits for the display left out. Its longest busy time is the frame's **critical path**, kept as
  the metric `frame:critical_ms`. The graphics card is not timed in it: it stays *unmeasured*.
- **Each agent.** The scheduler times every run of a family's installed layer, and the agent reports
  it (`TelemetryEvent::AgentReport`): the row it ran, its time per device, and the host time its level
  quoted on the run's inputs. The DCC keeps each agent's time as `agent:<name>_time_ms`, and its
  [cost book](../concepts/arbiter.md#the-cost-book) learns how far each row's quote is from it.
- **The GPU, inside the renderer.** The wgpu backend has a timestamp profiler
  (`crates/khora-infra/src/graphics/wgpu/profiler.rs`) that measures the **main pass** and the **frame
  total** on the graphics card, smoothed. Timestamps written in frame N are read at frame N + 2. The
  results land in the render system's `RenderStats` (`gpu_main_pass_time_ms`,
  `gpu_frame_total_time_ms`, with `draw_calls` and `triangles_rendered`), read through
  `RenderSystem::get_last_frame_stats`; no report carries them to the DCC.
- **Memory.** `SaaTrackingAllocator`, the global allocator of every binary, counts every heap
  allocation; the `MemoryMonitor` publishes `memory:current_bytes`,
  `memory:bytes_allocated_lifetime` and `memory:net_allocations`. The renderer's device reports the
  video memory the platform gives it and what it uses.
- **Pushed metrics.** Code that counts something holds a typed `CounterHandle`, `GaugeHandle` or
  `HistogramHandle` from the `MetricsRegistry`, registered once; the asset decoders record
  `assets:decode_time` and `assets:decoded_total`. Do not look a metric up by name in the hot path.

**Where it surfaces.** The editor's **Control Plane**, its second Spine mode — see
[Editor — The Control Plane](../reference/editor.md#10--the-control-plane):

- the **summary bar** — the frame time over the last frames, the frame budget, the processor's and
  graphics card's load, video memory, heap, and the budget multiplier;
- the **agents**, with the row each runs and its health;
- the **inspector**'s **Decision** card — what the selected agent was lent, the row planned and the
  row running, and why ([Debug a frame](./debug-a-frame.md) reads it step by step).

---

## Understand GORNA's decisions

GORNA decides *how* each agent runs; the [GORNA](../concepts/gorna.md) pages are the full reference.
For performance work, three pieces matter.

**The frame's target.** The heuristics (heat, battery, frame time, stutter, trend, processor, graphics
card, memory, death spiral) set a **setpoint**, 16.66 ms by default. The **frame governor**
(`khora_control::governor`) scales it by a **budget multiplier**, from 0.3 to 1.0, so the frames'
*measured* critical path settles 10 % under the setpoint. When frames run long the multiplier drops
and agents pick cheaper rows; when there is room it climbs back and agents upgrade. A safety ceiling
caps it at once on critical heat, a critical battery or memory at 95 % of its budget. See
[The arbiter — the frame governor](../concepts/arbiter.md#the-cost-model-and-the-frame-governor).

**Why an agent switched.** Select it in the Control Plane: the Decision card names the trigger that
opened the round, who decided, what bound the frame and which upgrades were refused. Every move of the
governor's multiplier opens a round, so a row change traces back to a trigger you can read. To walk one
decision step by step, use [Debug a frame](./debug-a-frame.md).

**Learnt costs anchor the quotes.** An agent quotes every row in its offer, but a quote is a formula
written on some machine. The DCC's **cost book** learns, for each agent and each row, the factor from
the quoted host time to the measured one, with its spread, by recursive least squares. A round plans in
measured time — a row whose runs vary a little higher — then lends each envelope back in the agent's
own quoted units, so the agent runs the row planned. A row never run borrows the mean of its agent's
learnt rows; an agent nothing was learnt of is planned at its quotes. What it learns is kept per machine
and game version as a profile, so the next run starts informed. See
[The arbiter — the cost book](../concepts/arbiter.md#the-cost-book).

---

## The fixed-timestep cost model

Rendering runs at the display's variable rate; the simulation advances in **fixed** increments so it
stays frame-rate independent and deterministic. The scheduler
(`crates/khora-control/src/scheduler/frame.rs`) reconciles the two with an accumulator
(`compute_sim_steps`).

**Sim steps scale with real dt.** Each frame the real wall-clock delta (clamped to
`MAX_FRAME_DELTA_SECONDS` = 0.25 s) is added to an accumulator, and whole game ticks of
`DEFAULT_FIXED_DELTA_SECONDS` (1/60 s) are consumed: `ticks = floor(accumulator / fixed_delta)`,
capped at `MAX_SIM_STEPS` (5 ticks, 83 ms). The leftover carries over and yields the render
`interpolation_alpha`. So a slow frame costs *more* sub-steps (more sim work), and a sustained
overload clamps to 5 ticks and drops the excess (slow-motion, never a freeze — the spiral-of-death
guard).

**The physics step is precision, not time.** The time the ticks advanced is handed to the families
whose place holds a fixed step (`Place::fixed_step`) — in practice physics — and their layer runs once
per whole step of its own size (`compute_sub_steps`), the remainder carried to the next frame. The
physics step is the game's `physics.step` setting (`PhysicsSettings`, 1/60 s by default), never a GORNA
lever, since the simulation's result depends on it:

| `physics.step` | Sub-steps per 60-FPS frame |
|---|---|
| 1/30 s | 1 every other frame |
| 1/60 s (default) | 1 |
| 1/120 s | 2 |

The step changes the *cost* of a second of simulation, never how many seconds pass: the cap counts
game ticks, so a finer step does not make a slow machine slower, and a script's `FixedUpdate` keeps
its 1/60-s cadence. With no fixed-step family, every family runs once per frame and the ticks are
still counted.

**Visual smoothness is decoupled from sim rate.** Because the renderer interpolates using
`interpolation_alpha`, a 30 Hz physics step still renders smoothly at the display rate. A finer fixed
step buys simulation accuracy (stiffer constraints, less tunneling), not visual smoothness — pick it
for correctness; GORNA never changes it under pressure. See
[Troubleshoot — Physics](./troubleshoot.md#physics).

---

## AGDF / data layout

AGDF adapts the *representation* of ECS data — never its meaning. See [AGDF](../concepts/agdf.md) for the
full model; for performance, two levers.

**The layout advisor (glass-box, ships today).** The DCC turns per-component access telemetry
(`query_count`, `rows_scanned`, `size_bytes`) into a **read-only** recommendation per component,
surfaced via `DccService::layout_recommendations()`:

```rust
pub enum LayoutRecommendation {
    KeepSoa,        // default column is fine
    SimdFieldSoa,   // lean component swept in large batches → field-SoA + SIMD
    HotColdSplit,   // fat component → split hot fields from cold
}
```

The DCC *observes and advises*; it never repacks (Data owns its layout). Online repack is the
deferred Layer 3 frontier.

**The SIMD field-SoA lever.** A *large, compute-bound, all-`f32`* hot component can opt into a
field-split column with `#[component(layout = "soa")]`, then be processed by the engine's `wide`-based
`f32x8` kernels (`khora_core::math::simd`). The benchmark
`crates/khora-data/examples/layout_bench.rs` measured, on the author's machine:

| Kernel | Layout | Result |
|---|---|---|
| `normalize_quat_batch` | in-place field-SoA | **4.25×** vs scalar |
| compute-heavy normalize | SoA + explicit `f32x8` | **4.1×** vs SoA scalar |
| compute-heavy normalize | AoSoA auto-vec | 2.9× vs SoA scalar |
| `compose_trs_to_mat4` | SoA in → AoS `Mat4` out | **0.9×** (a *loss*) |

The lesson is a **whole-pipeline property**: the SIMD win holds only while the data stays field-SoA
*resident*. The same `f32x8` math wins big in place but **loses** the moment it must scatter back to
an AoS result (the per-lane transpose dominates the cheap arithmetic). That is why persistent
field-SoA storage — not a per-frame transpose — is the real lever. Numbers are machine-dependent;
reproduce them with:

```text
cargo run -p khora-data --example layout_bench --release
```

**Rule of thumb.** Most components should stay AoS (`&T` queries, zero-copy). Reach for
`layout = "soa"` only for what the advisor flags as `SimdFieldSoa`.

---

## Flow view caching

Flows are read-only projectors: each Substrate Pass they re-derive a View (`RenderWorld`,
`ShadowView`, …) from the World and publish it to the `LaneBus`. Most frames nothing they read has
changed, so the projection is pure waste — and a cached View is *bit-identical* to a re-projected one,
so skipping the work changes only the HOW. See [AGDF — Flow view caching](../concepts/agdf.md) and
[ECS — Semantic domains](../concepts/ecs.md).

**What makes a Flow cache-hit vs re-project.** The `World` keeps a **change epoch** per
`SemanticDomain`: the tick of the latest write to one of its values or move of one of its rows
(spawn/despawn, component insert/remove, compaction). Handing out writable access moves nothing
until something is written, and `transform_propagation` and the physics writeback write only values
that differ, so a still scene keeps every cache. A flow that can name *all* of its inputs returns a `Flow::cache_key` combining the world
instance id, the relevant domain epochs, and any runtime fingerprints. The `register_flow!`
trampoline compares the key against the previous View and, on a match, republishes it (a cheap clone)
without re-running `select`/`project`. A `None` key disables (and clears) the cache.

Which flows opt in is an honest audit of their input signals:

| Flow | Cached? | Why |
|---|---|---|
| `AudioFlow` | yes | reads only Audio + Spatial epochs (+ instance id) |
| `RenderFlow` | yes | Render + Spatial epochs + a viewport-override fingerprint |
| `ShadowFlow` | yes | same inputs as `RenderFlow` |
| `UiFlow` | no | depends on surface size + hot-reloadable fonts (no epoch) |
| `PhysicsFlow` | no | the sim mutates Physics/Spatial every simulated frame — a cache would never hit |

So a cache-*hit* means "this domain did not change this frame": a static scene re-uses Views, while a
moving simulation re-projects. Representation-only changes (an AGDF layout repack) do **not** bump an
epoch, so they never invalidate a cached View.

---

## Practical tips

- **Always measure in `--release`.** Dev builds use `opt-level = 0` (dependencies at 2) and no debuginfo;
  release enables `lto` and `codegen-units = 1`. Per-frame numbers from a dev build are not
  representative.
- **Pre-size allocations in lanes.** Allocation churn is a real frame-hitch cause — the DCC raises a
  glass-box alert when resident-byte volatility is high (see [Telemetry](../concepts/telemetry.md)). Reserve
  buffers up front and reuse them across frames rather than allocating per frame.
- **Avoid per-frame churn in the hot path.** Hold typed `CounterHandle` / `GaugeHandle` handles instead of looking
  metrics up by string; keep field-SoA data resident across a SIMD loop ([§4](#agdf--data-layout)); do
  not transpose in and out.
- **Let GORNA pick the cheap path under pressure** instead of hard-coding a quality level — but pin it
  (`AdaptationMode::Manual`) while you A/B a specific change so adaptation is not a variable (see
  [Debug a frame](./debug-a-frame.md)).
- **Criterion benches.** The ECS query path has a criterion benchmark
  (`crates/khora-data/benches/data/query.rs`): run it with `cargo bench -p khora-data -- "ECS Queries"`. The AGDF
  layout numbers come from the `layout_bench` *example* ([§4](#agdf--data-layout)), not a criterion
  harness.

---

## What is not yet measurable

Being explicit about the gaps:

- **No GPU time in the frame report.** The frame graph is not timed on the graphics card, so the DCC
  sees the host's time only; a GPU-bound frame hides in the wait for the swapchain image. The
  renderer's timestamp profiler times the main pass and the frame total, not individual passes or
  materials, and does not reach the DCC.
- **No GPU time per lane.** The cost book corrects host time only; each row's GPU time stays as its
  lane quotes it.
- **No graphics-card load.** No shipped monitor reports it, so the GPU-load heuristic reads 0.
- **No memory pressure by default.** It needs a RAM budget (`DccConfig::memory_budget_bytes`), which
  the default configuration leaves unset.
- **No CPU tracing spans.** There is no span timeline (Tracy, `tracing`) wired up; the telemetry
  pipeline could feed one ([Telemetry — honest gaps](../concepts/telemetry.md#honest-gaps)).
- **No histogram export.** Histograms collect, but no export format is settled; metrics are read in
  the editor or in-process.

Recording and replaying GORNA's decisions is available: see [Debug a frame](./debug-a-frame.md).

---

*See also: [Telemetry](../concepts/telemetry.md), [GORNA](../concepts/gorna.md),
[The arbiter](../concepts/arbiter.md), [AGDF](../concepts/agdf.md), [ECS](../concepts/ecs.md),
[Editor](../reference/editor.md), [Troubleshoot](./troubleshoot.md), [Debug a frame](./debug-a-frame.md).*
