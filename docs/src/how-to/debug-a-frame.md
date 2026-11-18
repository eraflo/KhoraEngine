# Debug a frame / a GORNA decision

**Goal:** investigate a single frame — which agent runs which row, why the DCC lent it
the envelope it did, and how to pin or replay the decision so it stops being a moving
target. Assumes you know the [GORNA](../concepts/gorna.md) model and have the editor's
Control Plane open.

When to reach for this page: a row switched and you want the *reason*; quality
oscillates and you want to freeze it; or a bug only reproduces under one adaptation
sequence and you need it to recur. For the symptom-level "why did quality drop?",
start at [Troubleshoot](./troubleshoot.md); come here to go deeper.

---

## 1 — Read the Control Plane

The **Control Plane** is the editor's second Spine mode, a workspace of its own — see
[Editor — The Control Plane](../reference/editor.md#10--the-control-plane). It reads
the engine live:

| Region | What it shows |
|---|---|
| **Summary bar** (top) | The DCC at a glance: the budget multiplier (`budget×`), the families switched off, the frame budget and the frame time over the last frames, the processor's and graphics card's load ("—" while nothing measured the card), video memory, heap |
| **Agents** (left) | Every registered agent, grouped by importance, with the row it runs and its health |
| **Schedule** | The agents by execution phase. A schedule, not a timeline: per-phase timing is not measured |
| **Inspector** (right) | The selected agent: its timing and status, its health and message, and its **Decision** card |

The **Decision** card is the last GORNA decision for that agent, read from the DCC's
`DecisionWatch`:

- **Lent** — the envelope: its host time, its GPU time when it has one, its memory
  (*not limited* when the round knew no limit).
- **Planned row** and **Running row** — the row the DCC planned, and the row the agent
  runs. While a new row's layer is being prepared, the card says the agent is still on
  the old row and the swap lands at the next frame start.
- **Why** — *Opened by* (the trigger), *Decided by* (`Greedy`, `Exact search`, or
  *Floors only*), *Frame bound by* (the constraint that stopped the plan growing: host
  time, GPU time, a memory, a `Stable` ceiling), the *risk margin kept*, and each
  *upgrade refused* with the constraint it broke.
- **What the DCC learnt** — for each of the agent's rows, its learnt factor (×k), its
  spread (±σ) and the charge a switch to it would pay.

A decision read back from a journal is tagged *replayed · journal*; one the agent was
not lent says *Not lent in epoch N*.

For the numbers behind the card, the DCC keeps each agent's measured time as the
metric `agent:<name>_time_ms` and each frame's critical path as `frame:critical_ms`.

---

## 2 — Trace a decision back

A decision has four layers; read them top down.

### Step A — what opened the round

*Opened by* names the trigger. The [triggers](../concepts/gorna-protocol.md#epochs-and-triggers)
say who raises each. For `Pressure`, the DCC thread logs each heuristic's alert at
`info` as `DCC Analysis: <alert>` — the heat, a slow frame, a stutter, a rising trend,
a loaded processor, memory. Raise the log level of `khora_control` to see them outside
the editor.

### Step B — the target the round shared

The heuristics set a setpoint (16.66 ms by default, relaxed by heat or a low battery);
the **frame governor** scales it by the **budget multiplier** shown in the summary bar
(`budget×`). The DCC logs `DCC governor: multiplier=… (setpoint=…ms, N frames this
tick)` at `debug`, and each change as the alert `Governor: budget multiplier → x`.
Reading a multiplier:

- it decides only once 10 frames were reported, then only on frames measured after its
  last change was lent — 3 frames and 0.25 s — or at once after 3 frames in a row over
  the setpoint;
- one frame over twice the setpoint after a normal one is a hitch, left out;
- it holds in a band under its target, 10 % under the setpoint, and after lowering it
  does not raise past the multiplier that ran over, less 0.06, for 5 seconds: that is
  why it settles instead of cycling between two rows;
- it stops lowering when every agent already runs its floor;
- a **safety ceiling** caps it at once on critical heat (0.4), a critical battery or
  memory at 95 % of its budget (0.5);
- every move, either way, opens an epoch (`FrameGovernor`). Pressure drives the
  downgrades; this drives the *recovery*. That is the mechanism behind "it upgraded
  right after a downgrade".

A cost-model forecast over the setpoint tightens the target the round shares and opens
a round as `ForecastBreach`, with an alert naming the forecast. The full loop is in
[the arbiter](../concepts/arbiter.md#the-cost-model-and-the-frame-governor).

### Step C — how the arbiter planned

*Decided by*, *Frame bound by* and the *upgrades refused* are the arbiter's own
account (`Why`). An upgrade refused for host time on a GPU-light scene usually means
the frame's critical path, not this agent, is the limit; a large ×k means the agent's
quotes are far from what its runs measure on this machine, and the DCC plans with the
measurement. A plan refused by the shield is logged at `debug`:
`GORNA: the … proposal for epoch … is refused`. See [the arbiter](../concepts/arbiter.md).

### Step D — the death-spiral stop

If three or more pressures are active in one tick, the DCC logs `Heuristic: DEATH
SPIRAL detected (N simultaneous pressure sources)` at `error`, and the round is
`Safety`: every agent at its floor. Two or more stalled agents do the same, logged as
`GORNA: death spiral (N stalled agents)`. If you see either, the machine is saturated
on several axes at once.

### Step E — what the agent made of its envelope

The agent chooses its row within its envelope: the best quality whose quote fits. A
running row other than the planned one has a reason:

- **the swap is under way** — the new layer is prepared off the frame and lands at a
  frame start;
- **its quotes moved** — the running row's quote no longer fitted, or drifted by more
  than 10 %, and the agent chose again between rounds;
- **nothing fitted** — the floor runs, and its report says `over_budget`; three in a row
  open an `Overruns` round;
- **a policy** — a `Manual` pin, a `Bounded` range, a cap, or a `Stable` envelope.

Check the agent's mode before concluding "GORNA chose X".

---

## 3 — Pin a row to isolate behaviour

To remove adaptation as a variable, pin the agent through the DCC service. There are
four modes (`AdaptationMode`): `Learning`, `Manual(row)`, `Stable`,
`Bounded { min, max }`.

```rust
dcc.set_adaptation_mode(AgentId::new("shadows"), AdaptationMode::Manual(StrategyId::LowPower));
```

The next round reads it — a mode opens no round by itself — and the agent then runs
the pinned row whatever its envelope, except under a `Safety` allocation. Pinning one
agent isolates its contribution while the rest adapt. For each mode and the hints, see
[Control GORNA adaptation](./control-gorna-adaptation.md).

---

## 4 — Record and replay a decision

When a bug only reproduces under one adaptation sequence, record it and replay it. The
DCC journals each epoch into a `DecisionTrace`: its allocation, its `Why`, and
everything the round read — the offers, the reports, the target, the waves, the
policies, the hints, the corrections, the planning (`EpochRecord`,
`crates/khora-core/src/agent/gorna/decision_trace.rs`).

```rust
dcc.start_decision_recording();      // a fresh journal, from the next tick
// … reproduce the scenario …
let trace = dcc.stop_decision_recording();

dcc.replay_decisions(trace);         // lends the recorded allocations again,
                                     // one per DCC tick, each at a frame start
                                     // of its own, asking no agent for an offer
assert!(dcc.is_replaying());         // true until the journal runs out
dcc.stop_replay();                   // back to live rounds at any time
```

Once the journal runs out, a live round opens (`Refresh`). `recorded_decisions()`
returns the journal so far, safe to call at any time. To record a whole run without
code, set `KHORA_RECORD_DECISIONS=<file>` when you start the app; the journal is
written there at shutdown.

A journal can also be **decided again** off the engine, through the arbiter or through
other proposer sets, to see whether a decision was the arbiter's best:

```text
cargo run --release -p khora-sdk --example arbiter_eval journal.json
```

Replay is not an `AdaptationMode`: it is a capability of the DCC service, so agents
keep their own modes while a run is recorded.

---

## 5 — Quick reference

1. **Decision card** — the trigger, who decided, what bound the frame, the upgrades
   refused.
2. **Summary bar + `DCC Analysis:` logs** — which pressure fired.
3. **`DCC governor:` debug log** — the multiplier against the setpoint; any move of it
   explains a round.
4. **The agent's mode** — confirm a policy did not decide.
5. **Pin with `Manual`** — freeze the variable while you reproduce.
6. **Record, then replay** — when you need the exact sequence to recur.

---

*See also: [GORNA](../concepts/gorna.md), [The protocol](../concepts/gorna-protocol.md),
[The arbiter](../concepts/arbiter.md), [Profile performance](./profile-performance.md),
[Troubleshoot](./troubleshoot.md), [Control GORNA adaptation](./control-gorna-adaptation.md),
[Telemetry](../concepts/telemetry.md).*
