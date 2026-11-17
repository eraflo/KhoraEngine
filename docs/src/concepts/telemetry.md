# Telemetry

<p class="kp-lede">A driver keeps an eye on two things at once: the dashboard — speed, fuel, the engine's heat — and how the car answers each move of the wheel. Neither alone is enough to drive well. Telemetry is the engine's dashboard and its feel for the road: what the machine has left, and what each piece of work just cost.</p>

**Telemetry** is how the engine measures itself: where the readings come from, where
they go, and who acts on them. This page explains why it is a subsystem of its own
rather than a debugging aid, the two kinds of reading the DCC (the Dynamic Context
Core, which shares the frame's time out) takes in, and where each comes from. For the
steps to read the numbers and profile a frame, see
[How-to: profile performance](../how-to/profile-performance.md).

---

## Why telemetry is first-class

A self-tuning engine is only as smart as its inputs. The DCC shares each frame's time
through [GORNA](./gorna.md), and it can do better than a fixed configuration only *if
it can see* how long the work takes and what the machine has left: video memory, heap,
heat, battery. Take those readings away and the adaptive engine falls back to a fixed
configuration.

So telemetry is the engine's **nervous system** in the [SAA](./saa.md): monitors run
beside the work, the DCC reads them on its own thread, and turns them into budgets.
The same readings feed the editor's Control Plane, where the engine's decisions become
visible.

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../images/gorna/telemetry-loop.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>The situation.</strong> Monitors sample the machine: memory and video memory, polled once a second inside the frame; heat, battery and the processor's load, read about once a second on the DCC's own thread.</li>
<li><strong>The cost.</strong> Every run of an agent and every frame sends a report: what the work took, measured where it ran.</li>
<li><strong>The decision.</strong> Twenty times a second the DCC drains its inbox, runs its heuristics and its governor, and, when something changed, publishes an allocation the agents are lent at a frame start.</li>
</ol>
</figure>

## Reports and context

Two kinds of reading reach the DCC, on the same inbox, and they answer different
questions:

- **What the work costs** comes from the **reports** of the
  [GORNA protocol](./gorna-protocol.md), never from a monitor. Each agent reports
  every run (`TelemetryEvent::AgentReport`): the row it ran, the time it took on each
  device, the time its level quoted. The host reports every frame
  (`TelemetryEvent::FrameReport`): how long the frame kept each device busy, the waits
  for the display left out. The DCC's [cost book](./arbiter.md#the-cost-book) learns
  from the agents' reports, and its
  [frame governor](./arbiter.md#the-cost-model-and-the-frame-governor) regulates on
  the frames'. A device a report did not measure is *unmeasured*, never zero: the
  graphics card stays unmeasured until the frame graph is timed.
- **The situation the work runs in** comes from **telemetry**: memory, video memory,
  heat, battery, the processor's load, the data layer's access patterns. No agent
  reports these — they are facts about the machine and the data, not costs of work —
  and the [heuristics](./gorna.md#the-heuristics) read them to set the target the
  frame is shared against.

Time never travels through a monitor: a monitor sees the machine, a report sees the
work.

## Two collection styles

- **Polled monitors.** Readings with a current value — the heap, the video memory —
  are *pulled*: the telemetry service (`TelemetryService`) asks each registered
  monitor (`ResourceMonitor`) for its value once a second, at the start of a frame,
  and forwards what it read to the DCC's inbox. A monitor is a trait in `khora-core`;
  the implementations live in `khora-infra` because they call platform APIs.
- **Pushed metrics.** Code that counts something as it happens *pushes* it into a
  metrics registry (`MetricsRegistry`) through a typed handle — a counter, a gauge or
  a histogram — registered once, so the hot path never looks a metric up by name. The
  asset decoders record how long each decode took (`assets:decode_time`) and how many
  ran (`assets:decoded_total`).

Every reading is named by a namespace and a name (`MetricId`), printed
`namespace:name`: `memory:current_bytes`, `frame:critical_ms`,
`agent:<name>_time_ms`.

## The tracking allocator

Memory visibility comes from a **tracking allocator** (`SaaTrackingAllocator`)
installed as the global allocator in every binary. It wraps the system allocator and
counts allocations and bytes in atomic counters: a few atomic operations per
allocation, small but real.

The memory monitor (`MemoryMonitor`) publishes those counters —
`memory:current_bytes`, `memory:bytes_allocated_lifetime`, `memory:net_allocations` —
and the DCC acts on them:

- **memory pressure:** memory used over a RAM budget, when the host sets one
  (`DccConfig::memory_budget_bytes`; none by default, which leaves the signal at 0).
  Over 85 % it opens a round, over 95 % it counts as a pressure and the safety ceiling
  holds the budget multiplier at 0.5;
- **allocation churn:** when the memory used varies a lot (a coefficient of variation
  over 0.15), the DCC raises an alert pointing to a likely per-frame allocation
  hotspot. An alert only: it changes nothing.

## The hardware

Heat, battery and the processor's load come from a hardware monitor (`SysinfoMonitor`,
the battery read through `starship-battery`) that the DCC reads on its own thread,
about once a second. It is not one of the telemetry service's monitors on purpose:
reading the sensors takes milliseconds — far more the first time — and the telemetry
service runs inside the frame, where those milliseconds would show as a hitch every
second. The editor reads the loads from the DCC's context.

The graphics card's load has a place in the DCC's context and a heuristic of its own,
but no shipped monitor reports it: it reads 0.

## Video memory

The renderer's device is its own video-memory monitor. wgpu says nothing of a GPU's
memory, so the device asks the platform through its backend: DXGI's budget on DX12,
`VK_EXT_memory_budget` on Vulkan (the local heaps' sizes without it), the working set
Metal recommends, and `GL_NVX_gpu_memory_info` on NVIDIA's OpenGL. The answer is the
budget the system gives the process, which moves with the other programs on the GPU,
and what is used — the swapchain and the driver included, not only what Khora
allocates. It costs a few microseconds. Without an answer (WebGPU, OpenGL elsewhere),
the device reports its own count and no capacity.

A host hands the monitors its renderer builds to the engine (`HostMonitors`, put in
the runtime before the boot), and the telemetry service reports each with its kind.
From the video-memory report the DCC works out what is left — the developer's budget
(`DccConfig::vram_budget_bytes`) taken whole, else 90 % of the platform's budget — less
what is used. That is the graphics card's memory a GORNA round shares: an upgrade that
would hold more is refused.

## Honest gaps

- **No GPU time yet.** The frame graph is not timed on the graphics card, so the
  reports carry the host's time only, and a GPU-bound frame hides in the wait for the
  swapchain image (issue #292). The wgpu backend has a timestamp profiler of its own —
  the main pass and the whole frame — but no report carries it.
- **No GPU load.** As above: the heuristic has nothing to read.
- **No trace records.** The pipeline could feed an external tracing tool, but none is
  wired up.
- **No export.** Histograms collect, but no export format or retention policy is
  settled: the DCC reads the latest values, not a history.

---

## Formalisation

*This part states what the DCC derives from the readings.*

- **Memory pressure.** With *u* the memory used and *B* the RAM budget,
  *π* = clamp(*u* / *B*, 0, 1), and *π* = 0 when either is unknown.
- **Allocation churn.** Over the samples of `memory:current_bytes` the store keeps
  (at least 10), with mean *μ* and variance *v*: an alert when √*v* / *μ* > 0.15.
- **Video memory left.** With *u* the bytes used, *C* the platform's budget and
  *B*<sub>v</sub> the developer's: *M*<sub>gpu</sub> = *B*<sub>v</sub> − *u* when
  *B*<sub>v</sub> is set, else ⌊0.9 *C*⌋ − *u*, never below 0; unchanged when a report
  carries neither.
- **The frame's critical path.** For a frame report with busy times
  *t*<sub>d</sub> on the measured devices, *C* = max<sub>d measured</sub>
  *t*<sub>d</sub>; a frame with no device measured has none.

## Next steps

- [How-to: profile performance](../how-to/profile-performance.md) — read live
  metrics and investigate a slow frame.
- [GORNA](./gorna.md) — how the DCC turns these readings into per-frame budgets.
- [The frame](./the-frame.md) — the cold-path tick where telemetry is consumed.
- [Glossary](../reference/glossary.md) — monitor, metrics registry, tracking
  allocator, DCC.
