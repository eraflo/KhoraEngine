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

**What the engine collects.**

- **Frame timing.** The wgpu backend has a GPU timestamp profiler
  (`crates/khora-infra/src/graphics/wgpu/profiler.rs`) that measures the **main pass** and the
  **frame total** on the GPU, smoothed with an EMA. It uses a two-pass timestamp scheme with a
  frame-lag read model (timestamps written in frame N are read at frame N+2).
- **Per-agent cost.** The scheduler times every `agent.execute()` and publishes a
  `TelemetryEvent::AgentCost { id, n, time_ms }` each frame, where `n` is the workload size. This is
  what feeds the cost model ([§2](#understand-gornas-decisions)–[§3](#the-fixed-timestep-cost-model)).
- **Draw calls / triangles / render stats.** Agents and lanes push named counters and gauges through
  the `MetricsRegistry` (e.g. `render.draw_calls`, `physics.bodies_active`). Names are dot-separated;
  the registry is concurrent.
- **Memory.** `SaaTrackingAllocator` (installed as the global allocator in every binary) tracks every
  heap allocation into atomic counters; the `MemoryMonitor` surfaces them as `memory.current_bytes`,
  `memory.bytes_allocated_lifetime`, `memory.net_allocations`.
- **Hardware monitors.** `GpuMonitor`, `MemoryMonitor`, `VramMonitor` poll GPU utilization, heap, and
  VRAM.

**Where it surfaces.** The editor's **Control Plane** (the sixth Spine mode) is the primary surface —
see [Editor — The Control Plane](../reference/editor.md):

- **Lane Timeline** — per-subsystem execution windows.
- **GORNA Stream** — the live negotiation feed (who switched, and why).
- **Meters Wall** — frame time, GPU %, memory, agent budget, assets pending.

To read live metrics from your own game/UI, the well-known metric names are documented in
`crates/khora-telemetry/src/lib.rs` under `WELL_KNOWN_METRICS`. To add your own, hold a
`Counter` / `Gauge` handle in the agent or lane that owns it (do not look up by string in the hot
path).

**Honest gap.** The GPU profiler measures *main pass* and *frame total*; there is **no fine-grained
per-stage GPU breakdown** (per-pass, per-material) and **no CPU tracing spans** (Tracy-style) wired up
yet. Per-agent CPU cost is available (from `AgentCost`), but it is not yet sliced per render stage.
See [What is not yet measurable](#what-is-not-yet-measurable).

---

## Understand GORNA's decisions

GORNA chooses *how* each agent runs every cold-path tick; the [GORNA](../concepts/gorna.md) chapter is the
full reference. For performance work, three pieces matter.

**The frame budget.** Heuristics (thermal, battery, phase, frame-time, stutter, trend, CPU pressure,
GPU pressure, memory pressure, death-spiral) collapse into a single frame-time **target**. A **PID
controller** (`khora_core::control::pid`) then drives the `global_budget_multiplier` applied to every
agent's budget, closing the loop on *measured* frame time versus that target. When frames run long the
multiplier drops and agents pick cheaper strategies; when there is headroom it climbs back toward 1.0
and agents upgrade. A hard safety ceiling caps the multiplier immediately on `Critical`
thermal/battery or near-budget memory pressure.

**Why an agent up/downgraded.** Read the **GORNA Stream** panel — each switch prints its reason
("RenderAgent: LitForward → Forward+ — GPU pressure"). Re-arbitration fires when the PID multiplier
has moved by more than `PID_RENEGOTIATE_DELTA` (0.05) since budgets were last issued, so a strategy
change always traces back to a measured target/multiplier move. To walk a single decision step by
step, use [Debug a frame](./debug-a-frame.md).

**Calibration anchors quotes to reality.** Agents quote *static* cost estimates in `negotiate()`, but
reality drifts per machine and per scene. The DCC fits each agent's measured `(n, time)` samples into
a `CostModel` (`c·f(n)` over `{1, n, n·log n, n²}`) and, during arbitration, rescales each agent's
strategy options so the option matching its *current* strategy equals the measurement (factor clamped
to `[0.25, 4.0]`, ordering preserved). The budget fitting therefore reasons about **measured
milliseconds**, not worst-case quotes — a cold-start agent with no sample keeps its quote until it is
measured. See [AGDF — anticipatory budgeting](../concepts/agdf.md).

---

## The fixed-timestep cost model

Rendering runs at the display's variable rate; the simulation advances in **fixed** increments so it
stays frame-rate independent and deterministic. The scheduler
(`crates/khora-control/src/scheduler.rs`) reconciles the two with an accumulator
(`compute_sim_steps`).

**Sim steps scale with real dt.** Each frame the real wall-clock delta (clamped to
`MAX_FRAME_DELTA_SECONDS` = 0.25 s) is added to an accumulator, and whole fixed steps are consumed:
`steps = floor(accumulator / fixed_delta)`, capped at `MAX_SIM_STEPS` (5). The leftover carries over
and yields the render `interpolation_alpha`. So a slow frame costs *more* sub-steps (more sim work),
and a sustained overload clamps to 5 steps and drops the excess (slow-motion, never a freeze — the
spiral-of-death guard).

**Picking the fixed step.** The fixed step is the smallest `fixed_timestep` any registered agent
declares; in practice physics owns it via its GORNA strategy:

| Strategy | Fixed step |
|---|---|
| LowPower (`Simplified`) | 30 Hz (`1.0 / 30.0`) |
| Balanced (`Standard`) | 60 Hz (`1.0 / 60.0`) |
| HighPerformance (`Standard`) | 120 Hz (`1.0 / 120.0`) |

(From `PhysicsAgent::apply_budget`.) With no fixed-timestep agent, the loop degrades to "everything
once per frame" and `DEFAULT_FIXED_DELTA_SECONDS` (1/60) is reported.

**Visual smoothness is decoupled from sim rate.** Because the renderer interpolates using
`interpolation_alpha`, a 30 Hz physics step still renders smoothly at the display rate. A finer fixed
step buys simulation accuracy (stiffer constraints, less tunneling), not visual smoothness — pick it
for correctness, and let the cheaper step be a valid GORNA downgrade under pressure. See
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

**What makes a Flow cache-hit vs re-project.** The `World` keeps a monotonic **change epoch** per
`SemanticDomain`, bumped at every mutation that can affect that domain's semantic content
(spawn/despawn, component insert/remove, `get_mut`, mutable query construction, deserialization,
compaction). A flow that can name *all* of its inputs returns a `Flow::cache_key` combining the world
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

- **Always measure in `--release`.** Dev builds use `opt-level = 1` and `line-tables-only` debuginfo;
  release enables `lto` and `codegen-units = 1`. Per-frame numbers from a dev build are not
  representative.
- **Pre-size allocations in lanes.** Allocation churn is a real frame-hitch cause — the DCC raises a
  glass-box alert when resident-byte volatility is high (see [Telemetry](../concepts/telemetry.md)). Reserve
  buffers up front and reuse them across frames rather than allocating per frame.
- **Avoid per-frame churn in the hot path.** Hold typed `Counter` / `Gauge` handles instead of looking
  metrics up by string; keep field-SoA data resident across a SIMD loop ([§4](#agdf--data-layout)); do
  not transpose in and out.
- **Let GORNA pick the cheap path under pressure** instead of hard-coding a quality level — but pin it
  (`AdaptationMode::Manual`) while you A/B a specific change so adaptation is not a variable (see
  [Debug a frame](./debug-a-frame.md)).
- **Criterion benches.** The ECS query path has a criterion benchmark
  (`crates/khora-data/benches/query_bench.rs`): run it with `cargo bench -p khora-data`. The AGDF
  layout numbers come from the `layout_bench` *example* ([§4](#agdf--data-layout)), not a criterion
  harness.

---

## What is not yet measurable

Being explicit about the gaps:

- **No fine-grained GPU stage breakdown.** The profiler times the main pass and the frame total, not
  individual passes or materials.
- **No CPU tracing spans.** There is no Tracy/`tracing`-span timeline yet; the telemetry pipeline is
  described as *compatible* with such a hookup, but it is not wired
  ([Telemetry — open questions](../concepts/telemetry.md)).
- **Coarse workload size.** The cost model's `n` is the **global entity count**, not a per-domain
  workload — per-domain refinement is an open GORNA item
  ([GORNA — open questions](../concepts/gorna.md)).
- **No histogram export.** Histograms collect, but a Prometheus/OpenMetrics exporter is not committed;
  metrics are read in-editor or in-process.

These are forward-looking: the observation tunnel and metric registry are built to absorb them when
they land. (GORNA decision record/replay, once roadmap, is now available — see
[Debug a frame](./debug-a-frame.md).)

---

*See also: [Telemetry](../concepts/telemetry.md), [GORNA](../concepts/gorna.md), [AGDF](../concepts/agdf.md),
[ECS](../concepts/ecs.md), [Editor](../reference/editor.md), [Troubleshoot](./troubleshoot.md),
[Debug a frame](./debug-a-frame.md).*
