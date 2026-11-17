# The GORNA protocol

<p class="kp-lede">A busy kitchen at dinner. When the orders change, the head chef calls the stations together; each says which dishes it can make and how long each takes. The chef hands each station a slice of the evening, and the slices start with the next ticket, never halfway through a plate. Each station then picks the best dish it can make in its slice, and after every plate it says how long that plate took. This page is that conversation, message by message, as the engine holds it for every frame's time.</p>

[GORNA](./gorna.md) is the dialogue between the **DCC** (the Dynamic Context Core,
the part of the engine that watches the running game and shares out its time) and
the **agents** (one per subsystem: shadows, physics, audio, the interface, …).
This page states the protocol: its states, its messages, when a round opens, who
decides what, when a decision takes effect, what happens when something goes
wrong, and what always holds. How the DCC computes its decision is
[the arbiter](./arbiter.md).

A few words first, each used throughout:

- A **row** is one way an agent can do its work: one of its lanes at one level,
  named by a `StrategyId`. A shadow agent might offer low, medium and high.
- The **floor** is an agent's cheapest priced row: the one it runs when nothing
  else fits.
- An **envelope** is what the DCC lends one agent: a time on each device (the
  processor, the graphics card, …) and a memory on each device. Never a row.
- An **epoch** is one numbered round of the dialogue. An **allocation** is the
  DCC's decision for an epoch: every agent's envelope, and the developer's rules.
  It holds from its epoch until the next.
- A **report** is what an agent says after every run: the row it ran, and what
  that cost.

The rule that shapes everything: **the DCC decides how much; the agent decides
how.**

---

## A round, state by state

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../images/gorna/round.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>In force.</strong> An allocation holds. Every agent runs within its envelope and reports every run.</li>
<li><strong>Triggered.</strong> Something changed. The trigger waits until the epoch has lasted 3 ticks of the DCC; an emergency never waits.</li>
<li><strong>Asking.</strong> The DCC asks every agent of a family switched on for an offer, read from prices the agent already holds. An agent busy for a tenth of a second keeps its envelope.</li>
<li><strong>Deciding.</strong> The arbiter turns the offers into one envelope per agent, or into the emergency allocation when even the floors do not fit.</li>
<li><strong>Published.</strong> The allocation waits on the budget channel for the next frame start; a newer one published meanwhile replaces it.</li>
<li><strong>Lent.</strong> At the frame start the scheduler lends it to every agent at once. Each chooses its row within its envelope; the new epoch is in force.</li>
</ol>
</figure>

The DCC runs on its own thread, the **cold path**, at 20 ticks a second
(`DccConfig::tick_rate`). Each tick it drains its inbox, runs its heuristics and
its frame governor, and asks whether a trigger is due (`Epochs::due`). The frames
run on the **hot path**, at their own pace. The two never wait on each other: the
DCC publishes, and the frame takes what was published at its next start.

## The four messages

One round is one round trip. An agent can say everything at once — its whole
menu, priced — so nothing is bid twice. The types live in
`khora_core::agent::gorna::protocol`.

| Message | From → to | Carries | When |
|---|---|---|---|
| `OfferRequest` | DCC → agent | `epoch`; `target`, the frame's target time; `pressure`; `reply_by`, the deadline | when an epoch opens |
| `Offer` | agent → DCC | `rows`: each `OfferRow` with its `id`, its `cost` per device, its `quality` from 0 to 1, its `confidence` (`Measured`, `Quoted`, `Assumed`); the `floor`; the `current` row; its `health` | in reply, at once |
| `Allocation` | DCC → every agent | `epoch`; `trigger`; one `Envelope` per agent; `pins` and `allowed` rows, the developer's rules; `late`, the agents that keep their envelope; `lever`, whether a lower target can still take something back; `price`, the cutoff of the upgrades, for information | published at once, lent at the next frame start |
| `Report` | agent → DCC | the `row` it ran; `measured`, its time per device; `done`, the share of its work done; `health`; `over_budget`; `stalled`; `workload`, the host time its level quoted; `switch`, what preparing its layer took; a `message` | after every run |

A row's **confidence** says how far its price can be trusted: `Measured` for the
row that ran, `Quoted` for a row its lane priced on this frame's inputs without
running it, `Assumed` for a row not priced at all — its memory alone, no time. The
DCC plans no `Assumed` row.

The DCC sends a `pressure` of 0: the heuristics' pressure reaches a round through
its target instead. The host also sends one message of its own after every frame,
the `FrameReport`: how long the frame kept each device busy, the waits for the
display left out. The [frame governor](./arbiter.md#the-cost-model-and-the-frame-governor)
reads it.

## Epochs and triggers

An epoch opens on a **trigger** (`Trigger`). Each one names who raises it:

| Trigger | Raised when |
|---|---|
| `Initial` | the DCC's first round, as soon as an agent is registered |
| `Pressure` | a [heuristic](./gorna.md#the-heuristics) asks for a round: heat, battery, frame time, stutter, a rising trend, a loaded processor or graphics card, memory |
| `ForecastBreach` | the cost models forecast the frame over its target; the target the round shares is tightened to match, by at most half |
| `FrameGovernor` | the [frame governor](./arbiter.md#the-cost-model-and-the-frame-governor) moved the budget multiplier, either way |
| `AgentsChanged` | an agent came or went: a family switched on or off (`TelemetryEvent::ActivationChanged`), an agent registered or dropped |
| `AgentStalled` | an agent reports it is stalled, with a health under 0.5 |
| `Overruns` | one agent reports over budget 3 runs in a row (`DccConfig::overruns_in_a_row`) |
| `Refresh` | a replayed journal ran out: a live round opens for the engine as it is now |
| `Safety` | the emergency: see below |
| `PolicyChanged` | named by the protocol; the DCC raises it nowhere. A policy set with `set_adaptation_mode` or `set_hint` is read by the next round, whatever opens it |

**An epoch lasts at least 3 ticks** (`DccConfig::min_epoch_ticks`, 150 ms at
20 Hz), so the engine does not flap from one round to the next. The first trigger
raised waits for that minimum and a later one does not replace it; it is never
dropped, not even by a replay. `Safety` replaces whatever waits, and opens its
epoch on the tick it is raised.

A round becomes `Safety` whatever its trigger when the heuristics detect a **death
spiral** (three or more pressures at once), when two or more agents report they
are stalled, or when even the floors do not fit the frame.

## Who decides what

| Question | Who | How |
|---|---|---|
| How much time and memory? | DCC | one envelope per agent per epoch |
| When to decide again? | DCC | the triggers above, and the shortest epoch |
| An emergency? | DCC | a `Safety` allocation: every envelope is its agent's floor |
| What does a row cost? | the agent's lanes | a quote, worked out on the frame's inputs |
| How far is a quote from the truth? | DCC | the [cost book](./arbiter.md#the-cost-book) learns it, and plans with it |
| Which row runs? | agent | the best row whose quote fits its envelope |
| What did it cost? | agent | a report after every run |
| The developer's rules | developer, through the DCC | a pin, a set of allowed rows, a cap, a `Stable` envelope |

The DCC plans one row per agent to size the envelopes, but it sends only the
envelope; the row it planned goes to the journal and the editor. The envelope is
cut just under any better row, and lent in the agent's own quoted units, so the
agent, choosing by its own quotes, runs the row the DCC planned. That is the
arbiter's work: [the arbiter](./arbiter.md) explains it.

## Envelopes lent at a frame start

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../images/gorna/frame-start.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>Two clocks.</strong> The DCC ticks 20 times a second. A frame starts when the last one ends; here the second frame is long, a level loading.</li>
<li><strong>Published, not lent.</strong> Epoch 42 is decided on a tick in the middle of a frame. It waits on the budget channel; the frame runs on under epoch 41.</li>
<li><strong>The newest wins.</strong> Epoch 43 is published before any frame start took 42: it replaces it.</li>
<li><strong>Lent at the frame start.</strong> The scheduler takes 43 and lends it to every agent at once. No frame runs half under one epoch and half under another.</li>
<li><strong>A busy agent.</strong> One agent was busy at that instant. It runs that frame on what it holds, and is handed 43 at the next frame start.</li>
</ol>
</figure>

The allocation travels on the **budget channel** (`BudgetChannel`), the only
thread between the two paths. The DCC publishes; at each frame start the scheduler
takes the oldest publication waiting (`BudgetChannel::sync`) and lends it
(`lend_allocation`):

- **Every agent of a family switched on** is handed it, under its lock, before
  anything else locks it that frame. Each agent's `apply` receives its envelope,
  its pin and its allowed rows (`Allocation::lend_to`).
- **A newer live allocation replaces an older one** still waiting. A replayed one
  is published *in turn* instead: each is taken at a frame start of its own, in
  order, however long the frames.
- **An agent busy at the frame start** is handed what it missed at the next one,
  every replayed epoch in order, then the one in force.
- **An agent the allocation does not name** keeps what it holds.
- **An agent named with no envelope** — the round could not price any of its rows
  — runs at its own price, within its pin and allowed rows.
- **A family switched on again** after being off is first withdrawn
  (`Agent::withdraw`: what it was lent is dropped), then owed only what was taken
  after it came back.

A new envelope does not make the agent run at once. It wakes the agent
(`Wake::Allocated`), which chooses again; a new row's layer is prepared beside the
installed one and swapped in at a later frame start ([lanes are layers](#lanes-are-layers)).

## How an agent chooses

An agent holds, for every row of its menu, the last price it knows: its own run's
for the row running, a **quote** for the others — the lane prices itself on the
frame's inputs without running. Given its envelope, the agent picks
(`choose`, in `khora-agents/src/lane_agent/choice.rs`):

1. **A pin wins.** A row the developer pinned runs, whatever its price.
2. **The rows allowed and priced** are the candidates; a developer's `Bounded`
   range filters them.
3. **Among those whose quote fits the envelope** — time and memory, on every
   device — the one of best **quality**. A row's quality is its `Impact`'s (none
   1.0, minor 0.7, major 0.4) times the share of the due work it serves.
4. **Between equals,** each lane's dearest level — more of the same work: more
   voices, more lights — then the cheapest of those lanes, since two lanes of equal
   quality give the same result. The forward renderer while a scene has few lights,
   Forward+ once it has more than the forward pass shades.
5. **When none fits,** the floor runs, and the report says `over_budget`. An agent
   never overruns in silence.
6. **When nothing is priced yet,** the default level its registration names.

A row that failed is passed by until the next envelope is lent, and the agent falls
back to a cheaper row by itself. Its offer leaves out every row another lane's row
*dominates* — of no worse quality and no dearer on any device — since the agent
never runs it while that row stands: the DCC sees only the steps an envelope can
buy.

## Lanes are layers

The agent chooses; it does not run its lane frame after frame. Each family has a
fixed **place** in the frame, and on it lies one **layer**: one of its lanes,
prepared at one level. The scheduler applies the installed layers every frame
without calling the agents. A layer budgeted with `#[budget]` is lent the
envelope's time on its busiest device. [Agents and Lanes](./agents-and-lanes.md)
and [Families and levels](./families-and-levels.md) tell that part of the story.

An agent is woken (`Wake`) only for an event:

| Wake | When |
|---|---|
| `FirstFrame` | nothing is installed at its place yet |
| `Allocated` | an allocation was lent: a new envelope, pin or allowed rows |
| `QuoteUnfit` | the running row's quote stopped fitting the envelope |
| `QuoteDrift` | the running row's quote moved more than 10 % from the last full quote (`QUOTE_DRIFT`) |
| `QuoteAge` | the last full quote is older than 1 s (`QUOTE_MAX_AGE`) |
| `TargetChanged` | the target the family draws into changed format or samples: every quote was made on another |
| `LaneFailed`, `InputRefused` | the lane failed, or an input was refused |
| `PrepareFailed` | a new row's layer could not be prepared; the installed one runs on |

Woken, the agent chooses again. A new row's layer is prepared off the frame,
beside the installed one, and swapped in at a frame start at the same place. The
layer it replaces is kept ready for 3 allocations (`KEEP_EPOCHS`), so going back to
it prepares nothing. The first run of a new layer reports its preparation time
(`Report.switch`): what a switch of row costs.

## When things go wrong

- **A late offer.** An offer made after the request's `reply_by` keeps the agent's
  envelope; the allocation names it in `late`.
- **An agent busy through the round.** The DCC waits at most
  `DccConfig::agent_lock_timeout_ms` (100 ms) for an agent's lock. One busy that
  long keeps the envelope it holds, like a late one.
- **An agent busy at a frame start** is not waited for: it runs that frame on what
  it holds, and is handed what it missed at the next frame start.
- **An agent that arrives or leaves** — a family switched on or off, as the editor
  does with scripts, physics and audio at Play and Stop — opens an epoch
  (`AgentsChanged`). The switch lands at a frame boundary, and the DCC hears of it
  as `TelemetryEvent::ActivationChanged`: it keeps the families off in
  `DccContext.inactive` and forgets the reports it held, made with other families
  on. Only the families on are asked for offers and lent envelopes.
- **An emergency.** Three pressures at once — heat, a critically slow frame, a
  saturated processor, memory: the *death spiral* — two or more stalled agents, or
  floors that do not fit: the allocation is
  `Safety`, and each envelope is its agent's floor. It overrides the developer's
  pins and ranges — safety wins — and it names no row: every family has a floor.
- **Overruns.** When the running row's quote stops fitting, its layer wakes the
  agent (`QuoteUnfit`) and it steps down by itself within its envelope. Three
  over-budget reports in a row from one agent open an epoch (`Overruns`).
- **A stalled agent** with a health under 0.5 opens an epoch (`AgentStalled`).
- **A lane that fails or panics** fails its own family's run, never the frame. A
  panic is caught and logged, the place is made usable again, and the agent falls
  back to a cheaper row.
- **Stale reports.** A report made under an epoch that a later epoch already
  answered no longer counts toward `Overruns` or `AgentStalled`: an old overrun
  cannot open a new epoch.
- **An agent that priced nothing** is lent no envelope: the round knows nothing of
  its cost, and a zero-time envelope would starve it.

## The invariants

What holds at every frame, and where the code makes it hold:

| | Invariant | Where |
|---|---|---|
| I1 | One envelope per agent per epoch; one row per agent per frame. | `Allocation`, `choose` |
| I2 | The planned rows' critical path fits the target on every device, or the round is `Safety`. | the [shield](./arbiter.md#the-shield), admission |
| I3 | The planned memory fits each device whose memory is known. | the shield |
| I4 | The developer's rules hold: a pin, a `Bounded` range, a cap, a `Stable` ceiling. | `Plan`, the shield, `choose` |
| I5 | Safety comes first: a `Safety` allocation lends floors and overrides every rule. | admission |
| I6 | A decision is a pure function of its inputs, recorded with them and its reasons; a journal decides again what it recorded. | `decide`, `DecisionTrace` |
| I7 | No lane runs on the DCC's thread: an offer is read from prices the agent holds. | `Agent::offer` |
| I8 | An allocation applies at a frame start, to every agent at once. | `BudgetChannel`, `lend_allocation` |
| I9 | An epoch lasts at least 3 ticks, except `Safety`. | `Epochs::due` |
| I10 | A late or busy agent keeps its envelope, and is named. | `Allocation.late` |
| I11 | An agent runs a row whose quote fits its envelope, or its floor and says so. | `choose`, `Report.over_budget` |

---

## Formalisation

*This part is for readers who want the guarantees stated precisely.*

**Notation.** *A* is the set of families switched on for the epoch (the
[activation](../reference/glossary.md#activation)). Agent *a* offers rows *r* with
a quoted cost *ĉ*<sub>a,r</sub> ∈ ℕ<sup>D</sup> — integer microseconds on each
device *d* — a memory *μ*<sub>a,r</sub> and a quality *q*<sub>a,r</sub> ∈ [0, 1].
Its floor is *f*<sub>a</sub>. The allocation lends it an envelope
*b*<sub>a</sub> = (*t*<sub>a</sub>, *m*<sub>a</sub>), a time and a memory per
device. Write *c* ≼ *b* when *c* fits *b* on every device, in time and in memory.

**The choice** at frame *t*, from the fresh quotes *ĉ*<sup>t</sup>, with no pin:

> Let *F* = { *r* allowed : *ĉ*<sup>t</sup><sub>a,r</sub> ≼ *b*<sub>a</sub> } and
> *F*<sup>∗</sup> its rows of greatest quality. For each lane *ℓ*, let
> *r*<sub>ℓ</sub> be its dearest row in *F*<sup>∗</sup> (by busiest-device time).
> Then *r*<sub>t</sub> is the cheapest of the *r*<sub>ℓ</sub>, the first on a tie;
> when *F* is empty, *r*<sub>t</sub> = *f*<sub>a</sub>, flagged over budget.

**Dominance.** Row *r* of lane *ℓ* is dominated by row *s* of another lane when
*q*<sub>s</sub> ≥ *q*<sub>r</sub> and *ĉ*<sub>s</sub> ≼ *ĉ*<sub>r</sub>, one of the
two strictly or *s* listed first. Then *r* ∈ *F* ⇒ *s* ∈ *F*, and the choice prefers
*s*: a dominated row is never chosen, and the offer leaves it out.

**Propositions.**

- **(P-a) A frame fits when the prices are right.** Let *x*<sup>∗</sup> be the plan
  and *T*′ ≤ *T* the target it was shared against. If every agent's run costs what
  it quoted, the frame's critical path *C*(*r*<sub>t</sub>) ≤ *T*′.
  *Proof.* Each agent runs a row that fits its envelope. On each device *d*, the
  share lends every planned time *p*<sub>a,d</sub> the same factor,
  1 + 0.8 (*T*′ − *P*<sub>d</sub>) / *P*<sub>d</sub>, *P*<sub>d</sub> being the
  plan's critical path on *d*; a cap, a ceiling or the hold to the plan only lower
  it. A wave's slowest member stays its slowest, so
  *C*<sub>d</sub>(*r*<sub>t</sub>) ≤ *P*<sub>d</sub> + 0.8 (*T*′ − *P*<sub>d</sub>) ≤ *T*′. ∎
  (One exception, by design: an agent whose planned row is priced at 0 µs is lent
  the target itself, held under every better row. Its rows cost less than the
  measure's grain, and the bound counts them as 0.)
- **(P-b) Every agent switched on can run.** Its floor is admitted, or the epoch
  is `Safety` and its envelope *is* its floor. *Proof.* Admission places every
  floor before any upgrade; when the floors breach the target or the memory, the
  allocation is `Safety`. ∎
- **(P-c) The choice can be replayed.** *r*<sub>t</sub> is a pure function of
  *b*<sub>a</sub>, the quotes, the pin and the allowed rows: the same frame with
  the same quotes chooses the same row. ∎
- **(P-d) No frame mixes epochs.** Let *e*<sub>t</sub>(*a*) be the epoch agent
  *a* holds during frame *t*. The scheduler lends only at a frame start, and lends
  the allocation it took to every free agent of *A* at once; so two agents lent at
  the same frame start hold the same epoch, and an agent busy then holds its
  previous one until the next start. *Proof.* `lend_allocation` runs once per frame
  start, before any wave, and is the only caller of `Agent::apply`. ∎

When the prices are wrong — an agent quoted too low — P-a no longer holds for that
frame. The layer sees its own quote leave the envelope on the run that overran, its
agent steps down from the next frame start, and the 20 % of slack kept back absorbs
what is between.

**Layers.** *L*<sub>t</sub>(*a*) is the layer installed at *a*'s place during frame
*t*.

- *No frame without a layer:* a layer staged into an empty place is installed at
  once; afterwards *L*<sub>t</sub>(*a*) changes only at a frame start, from
  *L*<sub>t−1</sub>(*a*) to the staged layer.
- *The place is fixed:* every lane of a family has the family's outputs and phase,
  so a swap changes how the family works, never what it produces or when.
- *Bounded memory:* a layer put aside is released once *K* = 3 allocations have
  been lent, or as soon as the installed layer and the kept ones exceed the
  envelope's memory on a device.
- *A quiet frame costs the agent nothing:* it is called only for a wake.

**The cost of the protocol.** One round trip per epoch, and at most one epoch per
3 ticks outside `Safety`. The choice costs O(|menu|) per agent, and only when the
agent is woken: a quiet frame adds no agent call. What the arbiter costs is in
[its own formalisation](./arbiter.md#formalisation).

## Next steps

- [GORNA](./gorna.md) — the overview: why the engine negotiates, the two paths.
- [The arbiter](./arbiter.md) — how the DCC turns offers into envelopes.
- [Agents and Lanes](./agents-and-lanes.md) — what an agent chooses between.
- [How-to: debug a frame](../how-to/debug-a-frame.md) — watch a live round in the
  editor.
