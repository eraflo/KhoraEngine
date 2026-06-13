# Telemetry

The engine's nervous system: where measurements come from, where they go, and who
acts on them. This page explains *why* telemetry is a first-class subsystem rather
than a debugging afterthought, and how the DCC turns observations into decisions. It
is an explanation, not a recipe — for the steps to read metrics and profile a frame,
see [How-to: profile performance](../how-to/profile-performance.md).

---

## Why telemetry is first-class

A self-optimizing engine is only as smart as its inputs. The DCC negotiates frame
budgets through **GORNA**, but it can only make better decisions than a static
configuration *if it can see* frame time, GPU utilization, VRAM headroom, and heap
pressure. Take those readings away and the whole adaptive premise collapses to a
fixed config.

So telemetry is not bolted on — it is the **nervous system** of the
[SAA](./saa.md). Monitors run alongside the workload, a registry collects the
readings, and the DCC consumes them on its cold-path tick and turns them into budget
decisions. The same readings power the editor's control surface, where the engine's
decision-making becomes visible.

## Two collection styles

The pipeline has two complementary halves, reflecting two kinds of data:

- **Poll-based monitors.** Hardware-facing readings — GPU utilization and timings,
  heap and virtual memory, video memory — are *pulled*: the telemetry service asks
  each registered monitor for its current value. A monitor is a trait; the concrete
  implementations live in the backend crate because they call platform APIs, while
  the trait surface stays portable.
- **Push-based metrics.** Subsystems *push* named counters, gauges, and histograms
  into a metrics registry. Names are dot-separated by convention
  (`subsystem.thing.unit`), and the registry is concurrent so agents on different
  threads write without contention.

The split is principled: hardware state has a current value worth sampling, while
software events are produced where they happen and pushed.

## The tracking allocator

Memory visibility comes from a **tracking allocator** installed as the global
allocator in every binary. It wraps the system allocator and records allocation
counts and sizes into global atomic counters — a few atomic ops per allocation,
small but real, so benchmark builds can swap in the bare system allocator.

What makes it first-class is that the readings are *consumed*, not merely displayed.
The memory monitor publishes the counters into the DCC's store, and the DCC turns
them into decisions: a system-RAM budget derives a memory-pressure signal that sits
alongside thermal, CPU, and GPU pressure and can cap the global budget multiplier
near the ceiling; high volatility in resident bytes surfaces an alert flagging a
likely per-frame allocation hotspot; and the deferred layout-repack cost/benefit
gate reads memory pressure so it declines a repack — which transiently doubles a
column — under tight memory. Allocation tracking feeding adaptation directly is the
difference between a readout and a nervous system.

## The DCC closes the loop

The DCC's cold-path loop runs at a low frequency relative to the frame and does the
same cycle each tick: poll every monitor, read the named metrics it cares about,
feed the readings into its heuristics, arbitrate budgets, and send them out. The
hot path never looks metrics up by string — an agent that emits its own per-frame
metric holds a typed counter or gauge handle, and only the cold path and the editor
pay the string-keyed lookup, which they can afford.

```
hardware monitors  ┐
                   ├─→ telemetry service ─→ DCC (cold path)
pushed metrics     ┘        ↑                  │ heuristics → GORNA → budgets
                            │                  ↓
                    editor reads out      budget channel → agents
```

Telemetry → heuristic → budget. The loop closes through the engine observing
itself — which is the whole point of making telemetry a subsystem rather than a
side-channel.

## Honest gaps

The pipeline is real but not complete, and it is worth naming the edges:

- **No per-stage GPU breakdown.** GPU timings are coarse; the renderer does not yet
  attribute cost to individual passes.
- **No per-frame trace records.** Integration with an external tracing tool is
  compatible with the pipeline but not wired up.
- **Histogram export and retention are unsettled.** Histograms collect, but an
  export format and a long-term retention policy are not yet committed — the DCC
  reads the latest value, not a history.

These are roadmap edges, not design flaws; the seams exist for each.

## Next steps

- [How-to: profile performance](../how-to/profile-performance.md) — read live
  metrics and investigate a slow frame.
- [SAA](./saa.md) — how the DCC turns these readings into per-frame budgets.
- [The frame](./the-frame.md) — the cold-path tick where telemetry is consumed.
- [Glossary](../reference/glossary.md) — monitor, metrics registry, tracking
  allocator, DCC.
