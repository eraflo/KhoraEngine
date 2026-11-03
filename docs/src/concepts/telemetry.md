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
configuration *if it can see* how long the work takes and what the machine has
left: VRAM headroom, heap pressure, heat, battery. Take those readings away and the
whole adaptive premise collapses to a fixed config.

So telemetry is not bolted on — it is the **nervous system** of the
[SAA](./saa.md). Monitors run alongside the workload, a registry collects the
readings, and the DCC consumes them on its cold-path tick and turns them into budget
decisions. The same readings power the editor's control surface, where the engine's
decision-making becomes visible.

## Reports and context

Two kinds of reading reach the DCC, on the same inbox, and they answer different
questions:

- **What the work costs** comes from the **reports** of the GORNA protocol, never
  from a monitor. Each agent reports every run — the row it ran, the time it took
  on each device, the work it did — and the host reports every frame: the
  `FrameReport`, how long the frame kept each device busy, the waits for the
  display left out. The DCC fits its cost models on the agents' reports and
  regulates the frame on the frame's (see [GORNA](./gorna.md)). A device a report
  did not measure is *unmeasured*, never zero: the GPU stays unmeasured until the
  frame graph is timed.
- **The situation the work runs in** comes from **telemetry**: memory, video memory,
  heat, battery, the CPU's load, the data layer's access patterns. No agent reports
  these — they are facts about the machine and the data, not costs of work — and
  the heuristics read them to shape the target the budget is shared against.

Time, in particular, never travels through a monitor: a monitor sees the machine,
a report sees the work.

## Two collection styles

The pipeline has two complementary halves, reflecting two kinds of data:

- **Poll-based monitors.** Hardware-facing readings — heap and virtual memory,
  video memory — are *pulled*: the telemetry service asks
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

## The hardware

Heat, battery and the CPU's load come from a hardware monitor (`SysinfoMonitor`,
the battery read through `starship-battery`) that the DCC reads on its own thread,
about once a second. It is not one of the telemetry service's monitors on purpose:
reading the sensors takes milliseconds — far more the first time — and the
telemetry service runs inside the frame, where those milliseconds would show as a
hitch every second. The editor reads the loads from the DCC's context.

## Video memory

The renderer's device is its own video-memory monitor: it counts the bytes it
allocates. A host hands the monitors its renderer builds to the engine
(`HostMonitors`, put in the runtime before the boot), and the telemetry service
reports each with its kind. From the VRAM report the DCC works out what is left —
the developer's budget (`DccConfig::vram_budget_bytes`), else the device's capacity,
less what is used — and GORNA caps the upgrades that add video memory on it. wgpu
does not give a device's capacity, so without a budget nothing is capped yet; asking
the platform for it is the next step.

## The DCC closes the loop

The DCC's cold-path loop runs at a low frequency relative to the frame and does the
same cycle each tick: poll every monitor, read the named metrics it cares about,
feed the readings into its heuristics, arbitrate budgets, and send them out. The
hot path never looks metrics up by string — an agent that emits its own per-frame
metric holds a typed counter or gauge handle, and only the cold path and the editor
pay the string-keyed lookup, which they can afford.

```
hardware monitors  ┐
                   ├─→ telemetry service ─┐
pushed metrics     ┘        ↑             ├─→ DCC (cold path)
                            │             │     │ heuristics → GORNA → budgets
agents' and frame reports ──┼─────────────┘     ↓
                    editor reads out      budget channel → agents
```

Telemetry → heuristic → budget. The loop closes through the engine observing
itself — which is the whole point of making telemetry a subsystem rather than a
side-channel.

## Honest gaps

The pipeline is real but not complete, and it is worth naming the edges:

- **No GPU time yet.** The frame graph is not timed on the GPU, so the reports
  carry the host's time only and a GPU-bound frame hides in the wait for the
  swapchain image (issue #292).
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
