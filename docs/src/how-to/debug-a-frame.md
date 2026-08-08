# Debug a frame / a GORNA decision

**Goal:** investigate a *single* frame — figure out which agent chose which strategy, why GORNA made
that call, and how to pin or replay the decision so it stops being a moving target. Assumes you know
the [GORNA](../concepts/gorna.md) model and have the editor's Control Plane open.

When to reach for this page: a strategy switched and you want the *reason*; quality oscillates and you
want to freeze it; or a bug only reproduces under a specific adaptation sequence and you need it to
recur. For the symptom-level "why did quality drop?" entry, start at
[Troubleshoot — GORNA](./troubleshoot.md#gorna--adaptation); come here to go deeper.

---

## 1 — Read the per-frame telemetry

Open the editor's **Control Plane** (the sixth Spine mode). It is a first-class workspace, not a
profiler popup — see [Editor — The Control Plane](../reference/editor.md). Three regions matter for a single
frame:

| Region | What it shows | What to read it for |
|---|---|---|
| **Lane Timeline** (top) | Per-subsystem execution windows as colored bands | Which subsystem owns the frame's cost this tick |
| **GORNA Stream** (bottom-left) | Live negotiation feed: timestamp, subsystem, suggestion, accept/reject | *Why* an agent up/downgraded |
| **Meters Wall** (bottom-right) | Frame time, GPU %, memory, agent budget, assets pending | The signals that drove the decision |

The Lane Timeline shows execution *windows* per subsystem, not a per-render-stage GPU breakdown — that
finer slice does not exist yet (see [Profile performance — gaps](./profile-performance.md#what-is-not-yet-measurable)).
For the CPU side, the scheduler publishes a `TelemetryEvent::AgentCost { id, n, time_ms }` per agent
per frame, surfaced as the `agent.<Id>_time_ms` metric — that is your per-agent cost.

If you are reading metrics from your own code rather than the editor, the well-known names live in
`crates/khora-telemetry/src/lib.rs` under the metric names the DCC emits (built inline, e.g. `agent.<Id>_time_ms` in `DccService`); the DCC also ingests
`renderer.frame_time` (ms), `renderer.draw_calls`, and `renderer.triangles_rendered` from the GPU
report.

---

## 2 — Recognize which agent chose which strategy, and why

Adaptation is two stages: **the heuristics decide a target**, then **the PID + arbitrator turn that
target into a strategy per agent**. Reading a decision means tracing it back through both.

### Step A — read the reason off the GORNA Stream

Each switch in the stream prints its reason, e.g. `RenderAgent: LitForward → Forward+ — GPU
pressure`. That suffix is the heuristic that fired. The `HeuristicEngine`
(`crates/khora-control/src/analysis.rs`, `analyze`) evaluates these pressure sources every cold-path
tick, starting from a 60 FPS baseline target (16.66 ms):

| Pressure source | Trigger | Effect on the target |
|---|---|---|
| **Thermal** | `Throttling` / `Critical` | Relax to 33.33 ms (30 FPS) / 50 ms (~20 FPS) |
| **Battery** | `Low` / `Critical` | Relax to 33.33 ms / 50 ms |
| **Frame time** | avg over warn / critical threshold | Force negotiation (critical also counts as pressure) |
| **Stutter** | frame-time variance over threshold | Force negotiation |
| **Trend** | rising frame-time slope (`MetricStore::get_trend`) | Preemptive negotiation *before* a breach |
| **CPU / GPU load** | above critical (GPU also has a warn tier) | Force negotiation, counts as pressure |
| **Memory pressure** | resident RAM nears the developer-set budget | Force negotiation, counts as pressure |

The logs mirror the stream: the DCC thread emits `DCC Analysis: <alert>` at `info` for each alert in
the report, and `DCC PID: multiplier=… (setpoint=…ms, measured=…ms, dt=…s)` at `debug`. Raise the log
level on `khora_control` to see them outside the editor.

### Step B — understand the multiplier that scaled the budget

The heuristics produce a single `suggested_latency_ms` **target**. A **PID controller** then drives
the `global_budget_multiplier` so *measured* frame time tracks that target (`service.rs`, the DccService
cold-path loop). Key behaviours when reading a decision:

- The multiplier only updates once there are at least `FRAME_TIME_MIN_SAMPLES` frame-time samples and
  a non-zero measurement; before that it holds the PID's current output.
- It is clamped to a **hard safety ceiling** on `Critical` thermal/battery or near-budget memory — that
  cap is applied *after* the PID, so a critical signal overrides loop convergence immediately.
- A change in the multiplier of more than `PID_RENEGOTIATE_DELTA` (0.05) since budgets were last
  issued forces re-arbitration — **in both directions**. Pressure heuristics drive downgrades; this
  delta is what drives *recovery* (the multiplier climbing back toward 1.0 re-issues budgets so agents
  upgrade again). That is the mechanism behind "it upgraded right after a downgrade".

### Step C — recognize the death-spiral stop

If **three or more independent pressure sources** are active in the same tick, `analyze` sets
`death_spiral_detected` and logs `Heuristic: DEATH SPIRAL detected (N simultaneous pressure
sources)` at `error`. That is the emergency stop, not a per-signal downgrade — if you see it, the
machine is genuinely saturated on several axes at once.

### Step D — confirm the agent honoured (or overrode) the choice

Even with a target and a multiplier, the *final* strategy depends on the agent's `AdaptationMode`. An
agent pinned with `Manual` keeps its strategy regardless of the fit; a `Stable` agent refuses
opportunistic upgrades; a `Bounded` agent clamps to its range. Check the mode before concluding
"GORNA chose X" — the arbitrator may have *wanted* Y and been overridden. See the next section.

---

## 3 — Pin a strategy to isolate behaviour

To remove adaptation as a variable, set the agent's **`AdaptationMode`** through the DCC service.
There are exactly four modes (`crates/khora-core/src/control/gorna.rs`):

```rust
dcc.set_adaptation_mode(AgentId::Renderer, AdaptationMode::Manual(StrategyId::LowPower));
```

| Mode | Effect |
|---|---|
| `Manual(strategy)` | Pin the agent to one strategy; GORNA will not move it (a death-spiral safety stop can still force a downgrade). |
| `Stable` | Block opportunistic *up*-switches; the agent can still downgrade under pressure. |
| `Bounded { min, max }` | Clamp the negotiated range. |
| `Learning` *(default)* | Negotiate freely. |

`set_adaptation_mode` is thread-safe and takes effect on the next arbitration tick. Pinning a single
agent lets you isolate one subsystem's contribution while the rest adapt normally. For the broader
recipe and the trade-offs of each mode, see
[Control GORNA adaptation](./control-gorna-adaptation.md). For the symptom-driven version, see
[Troubleshoot — Pin a strategy](./troubleshoot.md#pin-a-strategy-so-adaptation-stops-surprising-you).

---

## 4 — Record and replay a GORNA decision

When a bug only reproduces under a *specific* adaptation sequence, capture that sequence and replay it
bit-for-bit. The DCC records its per-tick decisions into a `DecisionTrace` (a `Vec<TickDecisions>` in
arbitration order, `crates/khora-core/src/control/gorna.rs`) and can drive arbitration from a trace:

```rust
dcc.start_decision_recording();      // begins a fresh trace; takes effect next tick
// … reproduce the scenario …
let trace = dcc.stop_decision_recording();   // returns the captured DecisionTrace

dcc.replay_decisions(trace);         // each subsequent tick issues the recorded
                                     // strategies in order, bypassing live fit +
                                     // AdaptationMode, until the trace is exhausted
assert!(dcc.is_replaying());         // true until the trace runs out
dcc.stop_replay();                   // return to live arbitration at any time
```

During replay, arbitration drives every tick from the recorded decisions in order until the trace is
exhausted, then falls back to live negotiation — so a captured run reproduces exactly, independent of
the machine's current thermal/frame-time signals. `recorded_decisions()` returns a snapshot of the
trace captured so far (safe to call any time, e.g. to inspect mid-recording).

> **Note on `AdaptationMode::Replay`.** Replay is *not* an `AdaptationMode` variant — the four modes
> are `Learning`, `Manual`, `Stable`, `Bounded`. Record/replay is a separate `DccService` capability
> (the methods above), so you can record a run while individual agents still carry their own modes.

---

## 5 — Quick reference: what to look at, in order

1. **GORNA Stream** — read the switch reason (the `— <cause>` suffix).
2. **Meters Wall + `DCC Analysis:` logs** — confirm which pressure signal actually fired.
3. **`DCC PID:` debug log** — check `setpoint` vs `measured` and the `multiplier`; a move > 0.05
   explains a re-arbitration.
4. **Agent's `AdaptationMode`** — confirm the agent didn't override the fit.
5. **Pin with `Manual`** — freeze the variable while you reproduce.
6. **Record → replay** — when you need the *exact* sequence to recur.

---

*See also: [GORNA](../concepts/gorna.md), [Profile performance](./profile-performance.md),
[Troubleshoot](./troubleshoot.md), [Control GORNA adaptation](./control-gorna-adaptation.md),
[Editor — The Control Plane](../reference/editor.md), [Telemetry](../concepts/telemetry.md).*
