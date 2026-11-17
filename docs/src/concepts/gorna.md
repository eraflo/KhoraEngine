# GORNA: resource negotiation

<p class="kp-lede">A building site has one day of work to share out. The foreman asks every craftsman for a quote, hands each one an envelope of money, and each craftsman decides how to do the job with what is in it. In the evening, everyone says what they did and what it cost. GORNA is that dialogue, played between the parts of the engine for every frame's time.</p>

**GORNA** — *Goal-Oriented Resource Negotiation and Allocation* — is the dialogue
between the engine's **agents** (the parts that do the work: shadows, physics, audio,
the interface, …) and the **DCC**, the Dynamic Context Core that watches the running
game. The rule that shapes it fits in one line: **the DCC decides how much; the agent
decides how.**

This page is the overview: why the engine negotiates at all, the dialogue in brief,
the two paths it runs on, what the DCC watches, and how a developer steers it. Two
pages go deeper:

- [The protocol](./gorna-protocol.md) — the states of a round, its messages, its
  triggers, when a decision takes effect, the hard cases and the invariants.
- [The arbiter](./arbiter.md) — how the DCC decides: the cost book, the planning, the
  greedy pass and the exact search behind a shield, the frame governor, replay.

For pinning or bounding an agent in practice, see
[How-to: control GORNA adaptation](../how-to/control-gorna-adaptation.md); for
watching a live decision, [How-to: debug a frame](../how-to/debug-a-frame.md).

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../images/gorna/dialogue.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>Every run, a report.</strong> Each agent says which way of working it ran, how long it took on each device, and how much of its work it got done.</li>
<li><strong>Something changed.</strong> The shadows took longer than their quote said. The DCC opens a new <em>epoch</em>, a numbered round of the dialogue.</li>
<li><strong>A call for offers.</strong> The DCC asks every agent switched on for an offer, and tells them the frame's target time.</li>
<li><strong>The offers.</strong> Each agent answers with its menu: every way it can do its work (a <em>row</em>), with its cost on each device, its quality, how sure the price is, and its floor — the cheapest row that keeps the game right.</li>
<li><strong>The envelopes.</strong> The DCC shares the frame's time out and publishes one <em>envelope</em> per agent: how much time and memory it may use. Never a row. Every agent receives it at the same next frame start.</li>
<li><strong>Each chooses.</strong> Every agent picks the best row whose price fits in its envelope. If nothing fits, it runs its floor and says so in its report.</li>
</ol>
</figure>

---

## Why negotiate at all

A traditional engine assigns budgets at compile time: physics gets 4 ms, rendering
gets 12 ms, audio gets the rest. Those numbers are right on exactly one machine — the
developer's — and wrong everywhere else. A laptop on battery, a workstation and a
handheld do not share a frame budget, and the same machine does not share one between
a quiet menu and a crowded fight.

GORNA replaces the fixed split with a negotiation that follows the game. Agents say
what they *can* do at several costs; the DCC watches the running system and shares
out a budget that fits *this* hardware, *this* scene, *this* moment. One binary adapts
each frame to hold its frame rate, instead of a binary tuned for one target.

**Only agents negotiate in GORNA.** The data layer does not compete here: it tunes its
own memory layout ([AGDF](./agdf.md)), watched by the DCC but never driven by it, and
never bids for the frame's time. GORNA arbitrates strategy; AGDF arbitrates layout.

## The dialogue in brief

1. **Reports.** Every agent reports every run: the row it ran and what it cost.
2. **A trigger.** When something changes — heat, a slow frame, a family switched on,
   overruns — the DCC opens an **epoch**, a numbered round.
3. **Offers.** Every agent of a family switched on offers its menu, priced from what
   it already knows.
4. **A decision.** The DCC's [arbiter](./arbiter.md) shares the frame's time and
   memory out as one **envelope** per agent.
5. **Lent at a frame start.** The allocation reaches every agent at once, at the next
   frame start, never in the middle of a frame.
6. **A choice.** Each agent picks its best row whose price fits its envelope; if none
   fits, its floor, and it says so.

| The DCC decides | The agent decides |
|---|---|
| how much time and memory each agent gets | which of its rows runs |
| when to decide again | when to step down within its envelope |
| the emergency: every agent to its floor | what its lanes say a row costs |

[The protocol](./gorna-protocol.md) states each step precisely.

## Cold path and hot path

<div class="kp-figure-frame">

{{#include ../images/gorna/paths.svg}}

</div>

GORNA runs on two clocks. The **cold path** is the DCC's own thread, 20 ticks a
second: it drains its inbox, runs the heuristics and the frame governor, and, when a
trigger is due, runs a round and publishes its allocation. The **hot path** is the
frame: at each frame start the scheduler lends the newest allocation; the installed
**layers** — each family's chosen lane, prepared at its level — run at their places
every frame; an agent is woken only for an event, and prepares a new layer beside the
installed one.

The two paths touch only through the **budget channel** and the **reports**, and the
hot path never waits on the cold one. The DCC never touches an agent's envelope: it
publishes, and the frame lends. Offers are read from prices the agents already hold:
no lane runs on the DCC's thread.

Within a phase the scheduler groups the layers into **waves** and runs each wave
concurrently. Two layers share a wave only when their declared
[contentions](./agents-and-lanes.md) are disjoint. Because a wave is concurrent, its
cost is its **critical path** — its slowest member — not the sum of its members: the
scheduler publishes its waves, and GORNA shares the frame against the maximum within a
wave and the sum across waves. Over-declaring a contention is therefore not free: it
serialises a wave that could have run concurrently, and no test reddens.

## The heuristics

Each tick, the heuristic engine (`HeuristicEngine::analyze`) runs a battery of
independent heuristics, each a near-pure function of the telemetry it reads. Together
they set the **setpoint** — the frame time to aim for, 16.66 ms (60 frames a second)
by default — and say whether a round should open. A new heuristic adds to the others
without touching them.

| Heuristic | Reads | Effect |
|---|---|---|
| **Thermal** | the heat state | throttling: 30 frames a second at most (33.33 ms); critical: 20 (50 ms). Both a pressure |
| **Battery** | the battery level | low: 30 frames a second at most; critical: 20, and a pressure |
| **Frame time** | the frames' critical path, from 10 frames on | above 18 ms: a round opens; above 25 ms: a pressure too |
| **Stutter** | its variance | above 4 ms²: a round opens |
| **Trend** | its slope | rising faster than 2 ms a window: a round opens before it stutters |
| **CPU load** | the processor's load | above 95 %: a round opens, and a pressure |
| **GPU load** | the graphics card's load | above 90 %: a round opens; above 95 %: a pressure too |
| **Memory** | memory used against its budget | above 85 %: a round opens; above 95 %: a pressure too |
| **Allocation churn** | how much the memory used varies (its coefficient of variation) | above 0.15: an alert only, never a change |
| **Death spiral** | the pressures above | three or more at once: the `Safety` allocation, every agent at its floor |

Two of them wait on a reading: the memory heuristic is inert until a RAM budget is set
(`DccConfig::memory_budget_bytes`), and no shipped monitor reports the graphics card's
load yet, so the GPU heuristic reads 0 (see [Telemetry](./telemetry.md)).

A setpoint the heuristics relax is a ceiling on the frame rate a round plans for; the
[frame governor](./arbiter.md#the-cost-model-and-the-frame-governor) then sets how
much of it the agents are lent.

One firm constraint runs through all of this: **GORNA can never force a phase.**
Adaptation changes the HOW, never the WHAT.

## AdaptationMode — the developer-control surface

GORNA is a partnership, not an autocracy. How much latitude it has over an agent is
set per agent through **`AdaptationMode`** (`DccService::set_adaptation_mode`), and
each mode is a shape the round gives the agent's envelope:

- **`Learning`** *(default)* — full negotiation.
- **`Manual(row)`** — the row is pinned: the round plans it, and the agent is told to
  run it whatever its envelope.
- **`Stable`** — the agent keeps the envelope it holds while that still fits the frame
  with every other agent at its floor, and takes no upgrade; one that no longer fits
  becomes a ceiling it may not grow past. It does not flap from epoch to epoch, and it
  still steps down under pressure.
- **`Bounded { min, max }`** — the rows planned and chosen are limited to a range,
  ordered `LowPower < Balanced < HighPerformance < Custom(n)` by *n*.

A developer **hint** (`EngineHint`, `DccService::set_hint`) adds a weight
(`Prioritize`: the agent counts more or less in the share) or a cap (`Cap`: a ceiling
on its envelope's time, never below its floor).

A mode or a hint is read by the next round, whatever opens it: setting one does not
open a round by itself. The `Safety` allocation gives every agent its floor in any
mode — safety overrides developer control. Recording and replaying the DCC's decisions
is a capability of the DCC service, not a mode: see
[the arbiter's journal](./arbiter.md#the-journal-and-why).

---

## Formalisation

*This part is for readers who want the loop stated precisely; the protocol's and the
arbiter's guarantees are in their own pages.*

**The setpoint.** With *θ*<sub>heat</sub> and *θ*<sub>battery</sub> each 0, 33.33 or
50 ms as the table says, the setpoint is
*S* = max(16.66, *θ*<sub>heat</sub>, *θ*<sub>battery</sub>) ms.

**The emergency.** Let *p* count the pressures raised in a tick (critical or
throttling heat, critical battery, frames averaging over 25 ms, a processor over 95 %, a
graphics card over 95 %, memory over 95 %). The tick is a death spiral when *p* ≥ 3.

**When a round opens.** Let *k*<sub>0</sub> be the tick the last epoch opened. A
pending trigger opens epoch *e* + 1 at tick *k* when *k* − *k*<sub>0</sub> ≥ 3, or at
once when it is `Safety`; the first epoch opens at the first tick with an agent.

**The target a round shares.** *T* = *S* · *ρ* · min(*m*, *c*), where *ρ* ∈ [0.5, 1]
is the cost model's forecast ratio (1 when it forecasts no breach), *m* ∈ [0.3, 1]
the governor's multiplier, and *c* the safety ceiling (1, 0.5 or 0.4).

- **Stated in [the protocol](./gorna-protocol.md#formalisation):** a frame fits when
  the prices are right (P-a), every agent can run (P-b), the choice replays (P-c), no
  frame mixes epochs (P-d).
- **Stated in [the arbiter](./arbiter.md#formalisation):** the exact search never
  lends less than the greedy (A-1), what is lent keeps the rules and fits (A-2), a
  round is a pure function of its record (A-3), the planned row is the row chosen
  (A-4).

## Next steps

- [The GORNA protocol](./gorna-protocol.md) — the round, message by message.
- [The arbiter](./arbiter.md) — how the DCC decides.
- [How-to: control GORNA adaptation](../how-to/control-gorna-adaptation.md) — pin,
  bound, or free an agent.
- [How-to: debug a frame](../how-to/debug-a-frame.md) — watch a live GORNA decision.
- [Agents and Lanes](./agents-and-lanes.md) — what GORNA negotiates over, and lanes
  as layers.
- [AGDF](./agdf.md) — the layout adaptation the DCC watches but does not auction.
- [Glossary](../reference/glossary.md) — GORNA, DCC, epoch, envelope, AdaptationMode,
  frame governor, budget multiplier, death spiral.
