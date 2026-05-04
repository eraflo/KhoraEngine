# Telemetry

The nervous system. Where measurements come from, where they go, who reads them.

- Document — Khora Telemetry v1.0
- Status — Authoritative
- Date — May 2026

---

## Contents

1. Why telemetry is first-class
2. Architecture
3. The monitors
4. SaaTrackingAllocator
5. MetricsRegistry
6. The DCC consumes telemetry
7. For game developers
8. For engine contributors
9. Decisions
10. Open questions

---

## 01 — Why telemetry is first-class

A self-optimizing engine is only as smart as its inputs. If the DCC cannot see frame time, GPU utilization, VRAM headroom, heap pressure — it cannot make better decisions than a static configuration would.

So telemetry is not an afterthought. It is the **nervous system**. Monitors run alongside the workload, the registry collects readings, the DCC reads them every cold-path tick and turns them into budget decisions.

The same readings power the editor's *Control Plane* — the workspace where the engine's mind becomes visible. See [Editor design system](./design/editor.md).

## 02 — Architecture

```
Hardware monitors (GPU, Memory, VRAM)
  ↓ register with
TelemetryService
  ├─ MonitorRegistry (poll-based readings)
  └─ MetricsRegistry (push-based events)
       ↓
DCC reads at ~20 Hz → Heuristics → GORNA arbitration
       ↓
Editor reads any time → Control Plane panels
```

Two collection styles:

- **Poll-based monitors** — `GpuMonitor`, `MemoryMonitor`, `VramMonitor`. The TelemetryService asks them for the current value.
- **Push-based metrics** — agents and lanes push named counters/gauges through `MetricsRegistry`. The registry is queried any time.

## 03 — The monitors

| Monitor | Tracks |
|---|---|
| `GpuMonitor` | GPU utilization, frame timings, queue depths |
| `MemoryMonitor` | Heap (resident set), virtual size |
| `VramMonitor` | Video memory usage |
| `SaaTrackingAllocator` | Per-allocation heap tracking |

All implementations live in `crates/khora-infra/src/telemetry/` because they call platform APIs. The trait surface (what counts as a monitor) is in `khora-core` and `khora-telemetry`.

## 04 — SaaTrackingAllocator

`SaaTrackingAllocator` is a global allocator that tracks every heap allocation. Installed once at startup:

```rust
#[global_allocator]
static ALLOC: SaaTrackingAllocator = SaaTrackingAllocator::new();
```

It records counts, sizes, and (in debug builds) call sites. The DCC reads the totals to detect memory pressure trends; the editor's *Control Plane* shows the live curve.

The cost is small — atomic counters per allocation — but real. In benchmark builds, it can be replaced with the system allocator. The trait surface is `khora-core::memory`; the implementation is `khora-data::allocators`.

## 05 — MetricsRegistry

For per-subsystem metrics that the agents and lanes emit:

```rust
let metrics = ctx.services.get::<Arc<MetricsRegistry>>().unwrap();
metrics.counter("render.draw_calls").inc_by(123);
metrics.gauge("physics.bodies_active").set(42.0);
metrics.histogram("frame.duration_ms").record(15.7);
```

Counters, gauges, histograms. Names are dot-separated by convention. The registry is concurrent — agents on different threads can write without contention.

The DCC's heuristics read named metrics by string (cold path). The editor's panels read by string (out of band). Hot-path code does not query metrics by string — agents that need their own readings hold a `Counter` / `Gauge` handle.

## 06 — The DCC consumes telemetry

The cold-path loop (~20 Hz) does:

1. Poll each registered monitor.
2. Read named metrics from `MetricsRegistry`.
3. Feed the readings into the nine heuristics. See [GORNA](./08_gorna.md).
4. Arbitrate budgets.
5. Send through `BudgetChannel`.

Telemetry → heuristic → budget. The whole loop closes through the engine's own observation of itself.

---

## For game developers

Most game code does not emit telemetry. The engine handles its own.

If you want to measure something specific to your game (boss attack frequency, level transitions, save count), use `MetricsRegistry`:

```rust
let metrics = ctx.services.get::<Arc<MetricsRegistry>>().unwrap();
metrics.counter("game.bosses_defeated").inc();
metrics.histogram("game.player_health").record(self.player_health as f64);
```

The values appear in the editor's *Console* and *GORNA Stream* panels (with appropriate filtering).

To read live engine metrics from your game (FPS, frame time, GPU utilization for your own UI), the names are documented in `crates/khora-telemetry/src/lib.rs` under `WELL_KNOWN_METRICS`.

## For engine contributors

The split:

| File | Purpose |
|---|---|
| `crates/khora-core/src/memory/` | `Allocator` trait, allocation counters |
| `crates/khora-data/src/allocators/saa_tracking.rs` | `SaaTrackingAllocator` implementation |
| `crates/khora-telemetry/src/service.rs` | `TelemetryService`, lifecycle |
| `crates/khora-telemetry/src/metrics/` | `MetricsRegistry`, `MonitorRegistry` |
| `crates/khora-infra/src/telemetry/` | `GpuMonitor`, `MemoryMonitor`, `VramMonitor` |

Adding a metric: pick a clear name (`subsystem.thing.unit`), document it as well-known if it is engine-wide, hold a `Counter` / `Gauge` handle in the agent or lane that owns it. Do not look up by string in the hot path.

Adding a monitor: implement the `Monitor` trait, register with `MonitorRegistry::register` at startup, the DCC will start polling.

## Decisions

### We said yes to
- **Telemetry as a first-class service.** It feeds the DCC; without it, GORNA is blind.
- **Two styles (poll + push).** Hardware monitors are pulled; software metrics are pushed.
- **`SaaTrackingAllocator` as the default.** The cost is small; the visibility is enormous. Benchmarks can swap it out.
- **String-keyed metric registry.** The cold path can afford the lookup. The hot path holds typed handles.

### We said no to
- **A separate "telemetry agent."** Telemetry has no per-frame strategies. It is a service that runs continuously.
- **Hot-path string lookups for metrics.** Agents that emit per-frame metrics hold `Counter` / `Gauge` handles, not string names.
- **An external profiler-only dependency.** Khora's editor is the primary surface. External tools (Tracy, perf) work as well, but they are not the design point.

## Open questions

1. **Histogram exporter.** Histograms collect, but the export format (Prometheus, OpenMetrics) is not yet committed.
2. **Per-frame trace records.** Tracy integration would be valuable. The telemetry pipeline is compatible; the hookup is undecided.
3. **Telemetry retention.** The DCC reads the latest value. Long-term retention (for replay-after-incident analysis) needs a storage policy.

---

*Next: the SDK from a game developer's chair. See [SDK quickstart](./16_sdk_quickstart.md).*
