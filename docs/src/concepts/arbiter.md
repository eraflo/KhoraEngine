# The arbiter

<p class="kp-lede">Packing a car for a trip. Everyone's one essential bag goes in first. Then the extras, the ones worth most for the room they take, until the boot no longer closes; a quick look checks nothing heavy sits on the eggs, and you note why the cooler stayed home. Next time you remember that the tent takes more room than its label says. The arbiter packs the frame the same way, every time the DCC decides.</p>

The **arbiter** is how the DCC turns the agents' offers into envelopes, each round of
the [GORNA protocol](./gorna-protocol.md). It lives in `khora-control/src/gorna/`:
the `GornaArbitrator` holds what it learnt between rounds, and the `Arbiter` decides
one round. It works in six stations:

1. **Predict** — the **cost book** corrects each quote by what the runs measured.
2. **Plan** — the round looks beyond its frame: a margin for the frames' spread, a
   charge for every switch of row.
3. **Propose** — two **proposers**, the greedy pass and the exact search, each plan
   one row per agent.
4. **Shield** — every plan is checked against the frame's time, the memory and the
   developer's rules; a plan refused is dropped.
5. **Share** — the best plan accepted is lent as envelopes, in each agent's own
   units.
6. **Journal** — the decision is kept with why it was made, so it can be shown,
   recorded and decided again.

Beside them, the **frame governor** sets the target the round shares. Each part is
below; [why the arbiter is built this way](#why-this-design) comes after, and the
[formalisation](#formalisation) at the end states what it guarantees.

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../images/arbiter/pipeline.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>Predict.</strong> A row quoted 2.0 ms of host time, measured about 1.5 times that, is planned at 3.3 ms: its learnt factor, a little more while its runs vary.</li>
<li><strong>Plan.</strong> The target loses a margin read from the frames' spread. Switching a row is charged its preparation and its swap, spread over the 5 seconds a plan is meant to hold.</li>
<li><strong>Propose.</strong> The greedy pass plans quickly; the exact search starts from its plan and can only improve it.</li>
<li><strong>Shield.</strong> Each plan must keep every rule, fit the frame on every device and fit the memory. A refused plan is dropped.</li>
<li><strong>Share.</strong> The best plan is lent: each envelope its planned row plus a part of the slack, cut under the next better row, converted into the agent's own quoted units.</li>
<li><strong>Journal.</strong> The decision says who made it, why the round opened, what stopped the plan growing and which upgrades were refused.</li>
</ol>
</figure>

---

## The cost book

A lane prices its rows with a formula — a `cost()` written, or tuned, on some
machine — and the machine running the game is another. The **cost book**
(`gorna/cost_book.rs`) learns, for each agent and each of its rows, how far the quote
is from the truth: measured host time ≈ *factor* × quoted host time. That correction
is a `Correction`: its `factor`, its `spread` (how far the runs scatter around the
factor), and the `runs` it was learnt from.

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../images/arbiter/cost-book.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>Before any run.</strong> A row is planned at its quote. The first run after its layer was swapped in is left out: its caches and pipelines were cold.</li>
<li><strong>Learning.</strong> Each run moves the factor, and the band of its spread narrows. A row above the floor is planned at its factor plus 1.645 spreads: cautious while its runs vary.</li>
<li><strong>Never run.</strong> Another row of the same agent borrows the mean of the agent's learnt rows, with 0 runs of its own. An agent nothing was learnt of is planned at its quotes.</li>
<li><strong>Stopped running.</strong> A row unrun drifts back toward its agent's mean, halfway every 120 runs of its agent: a factor learnt on a bad run is tried again in time.</li>
</ol>
</figure>

Each run's report pairs the two numbers: `measured.cpu`, the host time the run took,
and `workload`, the host time its level quoted on the run's own inputs, in µs. The
book learns each pair by **recursive least squares** with forgetting — each run keeps
0.95 of its weight per newer one (`CostBookConfig::forgetting`) — so it follows a
machine that warms up or a scene that changes. Rules it keeps:

- **Only the host time is corrected.** A row's time on the graphics card and the
  other devices stays as quoted: the frame does not time each lane on the GPU.
- **The floor is planned at its factor alone.** The agent runs its floor whatever it
  is lent, so it is admitted at the time it is learnt to take; the caution of
  *z* spreads applies to the rows above it. *z* comes from the round's risk: 1.645
  for 5 %.
- **The prior fades.** A row starts from a spread of 0.5 (`prior_spread`), which
  weighs less with every run of the row, so a row run often stops being held back by
  it.
- **The round decides in measured time, and lends in quoted time.** The agent chooses
  by its own quotes, so each envelope's host time is divided by its planned row's
  factor before it is lent (`in_quotes::lend_in_quotes`): the agent then runs the row
  the DCC planned. A busy agent's kept envelope is corrected by the row it last said
  it runs.

What the book learns reaches the plan at the next round. When nothing else opens one,
the periodic refresh does (`Trigger::Refresh`, after `DccConfig::refresh_every_ticks`
with no round, 5 s at 20 Hz): a quiet engine still plans with what it learnt, and a
row's cost that drifted is planned at its new price.

The editor's [Control Plane](../reference/editor.md#10--the-control-plane) shows, for
the selected agent, each row's factor (×k), spread (±σ) and switch charge.

### Learnt across runs

What the book learns of a machine running a game is kept from one run to the next, as
a **profile** (`CostProfile`): each learnt row, by its agent's name, with its factor,
spread and runs.

- **When.** Read before the DCC starts and handed to its book
  (`DccService::seed_cost_book`); written every 5 seconds while the game runs, when it
  changed, and when it shuts down.
- **Where.** On a developer's machine — the editor, the sandbox —
  `~/.khora/profiles/<machine>/<game>-<version>.ron`. On a player's machine — a
  packed game — `<data>/<game>/khora/profiles/<machine>.ron`, `<data>` being
  `%APPDATA%`, `~/Library/Application Support` or `$XDG_DATA_HOME`
  (`~/.local/share`). The `<machine>` key is 16 hex digits hashed from the graphics
  adapter, the CPU's model and cores, and the system (`MachineIdentity::key`).
- **The key.** The hash is FNV-1a 64 over a fixed encoding of those fields, never
  Rust's `DefaultHasher`, so the same machine has the same key on every run and with
  every build of the engine. The adapter's fields include its driver and driver
  version, and the system's its version and build: a driver or system update gives
  the machine another key, so what was learnt is no longer found and learning starts
  again. The profile filed under the old key stays on disk, unused.
- **Whose.** Only a profile learnt by this engine version, running this game, on this
  machine (its key), is used; any other is logged and left. A player's boot with
  nothing learnt on this machine — the first one, or the first after an update —
  reads the profile the game shipped beside its `runtime.json` (`profile.ron`), if
  any. That one is checked for the engine version and the game only, not the
  machine: it was learnt on another one, and seeds the player's until
  the player's own runs take over.
- **How much it weighs.** A seeded row counts as many prior runs as it was learnt
  from, at most 20, so this machine's own runs take over within a second.
- **Who keeps them.** Only an app that keeps user files; `KHORA_PROFILES` names a
  folder for any other, so a test never writes the user's folders. Nothing is ever
  uploaded.

## Planning beyond the frame

A plan is meant to hold for a while, so the round weighs more than its own frame. The
planner (`gorna/planning.rs`) hands each round a `Planning`, kept in the journal:

- **A switch is charged.** Moving agent *a* off the row it runs costs
  *w*<sub>a</sub> × (*s* + *S*<sub>0</sub>) / *H* of weighed quality: *w*<sub>a</sub>
  its weight, *s* the row's preparation, *S*<sub>0</sub> = 250 ms the swap's
  transient (cold caches), *H* = 5 s the horizon a plan is meant to hold. *s* is
  0 for a row the agent ran within the last 3 epochs (its layer is kept); else the
  last preparation measured for that row (`Report.switch`); else the agent's mean;
  else 5 ms (`switch_prior`). The running row is free. An upgrade must gain more than
  it costs to make: at the defaults, about 5 % of the agent's weight.
- **A margin.** The plans share the target less *z* × 1.4826 × the median absolute
  deviation of the recent frames' critical paths — the frames' spread, read so a
  hitch hardly moves it. It is read over the last 120 frames once 10 ran; a load
  (3 frames in a row past the target and the margin) starts the count again at the
  next frame back under the target. The margin never takes the target below what the
  admitted plans already need, and never makes a round `Safety`.
- **Growth, off by default.** The round can plan each row above the floor at its cost
  × the growth its agent's workload trend predicts over *H*
  (`PlanningConfig::follow_trends`). A trend read over a few reports follows a burst
  as readily as a real rise, so the DCC does not follow trends unless told to.

## How the DCC shares the time

Every proposer starts from **admission** (`gorna/admission.rs`). Each agent is placed
at its floor, and the floors must fit the frame's **critical path**: agents that run
side by side, in one wave, count for the slowest of them, waves add up, transfers
between the processor and the graphics card count, and agents at the background phase
count for nothing. If the floors do not fit — or the round opened as `Safety` — the
allocation is `Safety`, every envelope a floor. A `Stable` agent keeps the envelope it
holds while that still fits with every other agent at its floor.

The first proposer, the **greedy pass** (`arbiter/greedy.rs`), then grows the plan:

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../images/gorna/share.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>The floors first.</strong> The DCC adds up every agent's floor on the frame's <em>critical path</em>: agents that run side by side, in one wave, count for the slowest of them; transfers between the processor and the graphics card count too. If the floors do not fit, it is an emergency.</li>
<li><strong>Then the upgrades.</strong> Of every possible step up — one agent moving to its next row — the DCC takes the one that gains the most quality for each microsecond, weighed by how important the agent is, and starts again.</li>
<li><strong>Passed over.</strong> An upgrade that no longer fits is skipped; a smaller one, for another agent, may still fit.</li>
<li><strong>The slack.</strong> What is left is not thrown away: 80 % of it is shared out to the envelopes, in proportion to what each was planned; 20 % stays back for spikes. An envelope is at least the price of the row the DCC planned for that agent, and has room to absorb a small rise.</li>
</ol>
</figure>

Each step moves one agent up its menu's convex hull: of the rows of more quality and
no less cost, the one gaining the most weighed quality — less the change in switch
charge — per microsecond on its busiest device. A step that breaks a constraint is
undone, recorded as rejected with the constraint it broke, and that row ruled out;
a smaller step of the same agent may still fit. Ties go to the larger gain, then the
agent named first, then the lower row.

An agent's **weight** is its family's importance — Critical 1.0, Important 0.75,
Optional 0.5 — or a developer's `Prioritize` hint. A row's **quality** comes from its
`Impact`: none 1.0, minor 0.7, major 0.4. A family never has to invent a number.

## The exact search

The greedy pass is quick and usually right, but a knapsack can fool it: a cheap,
steep step taken first can leave no room for a large one worth more.

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../images/arbiter/exact.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>The floors.</strong> Two agents of equal weight share 9 ms. Their floors take 2 ms and are worth 0.8.</li>
<li><strong>The greedy's steepest step.</strong> Shadows to medium: 0.3 for 1 ms, against 0.6 for 7 ms for dense foliage. Worth 1.1.</li>
<li><strong>Stuck.</strong> With shadows at medium, dense foliage would take the frame to 10 ms. The greedy never goes back, and stops at 1.1.</li>
<li><strong>The exact search.</strong> Shadows low and foliage dense take exactly 9 ms and are worth 1.4: that plan is lent.</li>
</ol>
</figure>

The **exact search** (`arbiter/exact.rs`) is the arbiter's last proposer, and its
fallback for quality. It is a branch-and-bound over each agent's rows:

- **It starts from the greedy's plan,** so it never lends less.
- **It tries each agent's candidates best first:** its admitted row and every row its
  rules allow; an agent keeping an envelope has its admitted row only.
- **It cuts** a branch that cannot beat the best plan found even if every agent left
  took its best row, and one that breaks the frame with every agent left at its
  cheapest.
- **It stops at a node budget,** 3 072 nodes (`DEFAULT_NODE_BUDGET`), keeping the best
  plan found. The decision is bounded by a count, not a clock, so tests pin it.
- **It is deterministic:** a fixed order, and on equal worth the plan found first.

Of the plans the shield accepts, the arbiter lends the one worth most — its weighed
quality less its switch charges — and the greedy's on a tie. Its fallback for
safety is the floors: with no plan accepted, every agent keeps the row admission gave
it, decided by `Floors`.

## The shield

Whatever a proposer is — the greedy, the exact search, a learned policy one day —
its plan reaches the frame only through the **shield** (`arbiter/shield.rs`).
`Shield::check` refuses a plan:

- **that plans a row the rules rule out:** any row but the pin for a pinned agent,
  a row outside a `Bounded` range or over a cap, an unpriced row, a row over a
  `Stable` ceiling, or any move of an agent that keeps its envelope (late, busy, or
  `Stable`);
- **whose critical path is over the planned target** on any device;
- **whose memory is over what a device has,** where that is known.

A refused plan is logged at debug level and named in the decision; the next
proposer's plan is weighed instead. The shield checks the same planned problem the
proposers solve: each row at the cost the round planned, against the target less the
margin.

## Lending the plan

The plan becomes envelopes (`gorna/share.rs`):

- **The share.** Each envelope is the planned row's cost plus its part of 80 % of the
  slack — what the plan leaves of the target — in proportion to its planned time,
  device by device; memory the same way. 20 % stays back for spikes. A developer's
  cap and a `Stable` ceiling bound the time, never below the floor. A memory the DCC
  does not know limits nothing.
- **The hold.** An agent runs the best row its envelope fits, so an envelope that
  would fit a row of more quality than the planned one is cut just under it, on the
  device where that row costs most above the planned one (`hold_to_plan`). Without
  the hold, the slack would let the agent take an upgrade the plan refused.
- **In the agent's units.** The envelope's host time is converted into the agent's
  quoted units and held again at its raw quotes (`lend_in_quotes`).
- **The lever.** `Allocation.lever` says whether some unpinned agent runs above its
  floor: whether a lower target could take something back. The
  [frame governor](#the-cost-model-and-the-frame-governor) lowers only with one.

## The journal and why

Every decision carries its reasons, a `Why`:

- `by` — what decided it: `Greedy`, `Exact`, or `Floors` when no plan was accepted
  (`DecidedBy` also names `Planner`, a proposer slot no engine proposer fills:
  planning beyond the frame is part of the problem every proposer solves);
- `trigger` — why the round opened, `Safety` when the floors did not fit;
- `binding` — the constraint that refused the last upgrade tried: `Time(device)`,
  `Memory(device)`, or `Ceiling(device)` for a `Stable` policy;
- `rejected` — every upgrade found not to fit, with the constraint it broke.

After each round the DCC publishes a `DecisionView` on its `DecisionWatch`: the
epoch, the trigger, the target, the `Why`, each envelope as lent, the row planned for
each agent, the corrections and the planning. The editor's Control Plane reads it.

**Recording.** `DccService::start_decision_recording` journals every epoch into a
`DecisionTrace`. An `EpochRecord` keeps the allocation, the `Why`, and everything the
round read: the raw offers, the latest reports, the target, the waves, the policies
and hints, the heuristics' alerts, the importances, the corrections, the planning, the memory limits, the
agents at the background, the envelopes in force when it opened, the time, and a
summary of the frames since the last epoch. Any app records from its start when
`KHORA_RECORD_DECISIONS=<file>` is set: the journal is written there as JSON at
shutdown.

**Replay, in the engine.** `DccService::replay_decisions` lends a journal's
allocations again, one per DCC tick, each at a frame start of its own, asking no
agent for an offer. Live rounds resume once it runs out (`Trigger::Refresh`).

**Replay, through an arbiter.** Every input of a round is in its record, and the
decision is a pure function of them, so a journal can be **decided again**:
`khora_control::evaluation::replay` runs each recorded round through a set of
proposers (`Arbiters`: `Greedy`, `GreedyExact`, `Planned`, the engine's own, and
`PlannedOnQuotes`, without the cost book) and compares them. A journal decided again
by the arbiter that recorded it gives back every allocation. The `arbiter_eval`
example prints the comparison, over four simulated scenarios — a light ramp, shadow
casters, a script load, heat — and over a journal when given one:

```text
cargo run --release -p khora-sdk --example arbiter_eval [journal.json]
```

## The cost model and the frame governor

Two pieces set the **target** a round shares, and the split between them is the heart
of the control loop.

The **heuristics** ([GORNA: the heuristics](./gorna.md#the-heuristics)) set a
**setpoint**: the frame time to aim for, 16.66 ms by default, relaxed to 33.33 ms or
50 ms by heat or a low battery. The **frame governor** (`khora_control::governor`)
then scales the share by a **budget multiplier**, from 0.3 to 1.0, so the frames'
measured critical path settles under the setpoint. The round shares *setpoint ×
multiplier*.

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../images/arbiter/governor.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>Over.</strong> Frames run at 20 ms against a setpoint of 16.7 ms. The governor aims 10 % under it, at 15 ms.</li>
<li><strong>Down, fast.</strong> The multiplier is scaled by target ÷ estimate, 15 ÷ 20: from 1.0 to 0.75, never more than 30 % at once. The agents step down.</li>
<li><strong>Hold.</strong> The change waits for an epoch to lend it, then for 3 frames. The frames settle at 14 ms, inside the band under the target: the governor holds.</li>
<li><strong>Up, slowly.</strong> The scene lightens. Each decision raises the multiplier a quarter of the way to its goal, and for 5 s not past the multiplier that ran over, less 0.06.</li>
<li><strong>A hitch.</strong> One frame over twice the setpoint after a normal one is a load or a stall, nothing a row could fix: it is left out.</li>
</ol>
</figure>

The governor is a **ratio law**, the shape engines ship for dynamic resolution, not a
PID: the loop moves by whole rows, a frame or two late, on measurements with long
hitches, and those three things defeat a textbook PID. Its estimate is a low-pass over
0.25 s of frame time, weighted by each frame's length. A frame's time is its critical
path: the longest of the times it kept each device busy, the graphics card's included
when the renderer times it. Around it (`GovernorConfig`):

- **a band** 15 % deep under the target, where it holds, and **a memory**: after
  lowering, it does not raise past the multiplier that ran over, less 0.06, for 5
  seconds of frame time. Together they stop it cycling between two neighbouring rows;
- **fast down, slow up:** a lowering takes at most 30 % at once, a raise a quarter of
  the way, and no change smaller than 0.02 is made;
- **decisions only on frames measured after the last change was lent** — 3 frames and
  0.25 s — or at once after 3 frames in a row over the setpoint. A frame names the
  epoch it ran under, and one run before the epoch that lent the change is left
  out, however late its report comes (a GPU reading comes about two frames late);
- **hitches left out:** a frame over twice the setpoint after one within it;
- **no lever, no lowering:** when every agent already runs its floor, lowering would
  give nothing back, so it stops.

On top sits a **safety ceiling** (`safety_ceiling`) that caps the published multiplier
at once: 0.4 when the heat is critical, 0.5 when the battery is critical or memory is
at 95 % of its budget. The governor regulates the steady state; the ceiling handles
emergencies. Every move of the published multiplier, either way, opens an epoch
(`Trigger::FrameGovernor`): pressure drives the downgrades, and this drives
**recovery** — once frames have room, the multiplier climbs, envelopes grow, and agents
upgrade again.

The **cost model** forecasts. For each agent the DCC fits a `CostModel`,
*c* · *f*(*n*) over *f* ∈ {1, *n*, *n* log *n*, *n*²}, on its reports: *n* is the
run's quoted host time, the time its own measured run took. Read at each agent's
latest workload and summed over the waves, it forecasts the frame: each wave counts
for its slowest member, an agent the published waves do not name counts as a wave of
its own, an agent at the background counts for nothing, and one that reported no
workload yet is left out. A forecast over
the setpoint tightens the target the round shares — by at most half — and opens an
epoch (`ForecastBreach`). It tightens the share, never the governor's setpoint, so one
overrun is not cut twice.

## Why this design

The arbiter answers a question every adaptive engine meets: *given what each part can
do at what cost, which way should each part work this frame?* What follows is the
reasoning behind each choice, and the prior art it stands on.

**The problem is a knapsack.** Each agent picks one row from a menu, each row has a
cost and a value, and the total must fit: a *multiple-choice knapsack*, NP-hard in
general. Real-time systems met it first. **Q-RAM** (Rajkumar, Lee, Lehoczky and
Siewiorek, RTSS 1997) shares a resource between applications offering several quality
levels by climbing each one's convex hull in order of marginal utility per unit of
resource. It is near-optimal and runs in microseconds: the greedy pass is Q-RAM, on
the frame's critical path rather than a single resource. Its known weakness is the
knapsack trap shown above, so an exact **branch-and-bound** follows it — seeded by
its plan, so it can only improve, and bounded by a node count, so a test can pin it
without timing a machine.

**Proposers behind a shield.** *Safe reinforcement learning via shielding*
(Alshiekh et al., AAAI 2018) lets a learner propose freely and a checker veto what is
unsafe. The arbiter takes the same shape without the learner: whatever proposes, the
frame is guarded by one small, checkable piece. Adding a proposer — a learned one
included — cannot break a rule the shield checks.

**Envelopes, not rows.** Operating systems split the work the same way: in
hierarchical scheduling (Linux `cpu.max`, `SCHED_DEADLINE`) the parent decides *how
much* time a group gets per period and the group decides *how* to spend it. The DCC
sees offers 20 times a second; an agent sees fresh quotes every frame. Lending time
rather than naming a row lets the agent react between rounds, while the hold keeps
it on the row the DCC planned when the quotes are right.

**A charge for switching.** Unreal's dynamic resolution moves only when the change is
worth it (`ChangePercentageThreshold`): adaptation without hysteresis flaps. Khora's
threshold is not a tuned percentage but measured times: what preparing the row took,
and the swap's transient, over the horizon.

**A learnt correction, row by row.** Unreal's Animation Budget Allocator plans with a
prior cost until it has measured one. Khora's quotes are such priors, and the cost
book is the measurement: recursive least squares with forgetting, the textbook
estimator for a slowly drifting gain, one parameter per row. It corrects host time
only, because host time is what each run measures.

**A margin for the spread.** "The frame runs over at most 5 % of the time" is a chance
constraint, P(frame > target) ≤ ε. Under a normal spread it becomes *target − z σ*;
the spread is read as 1.4826 × the median absolute deviation, which equals σ for a
normal spread and barely moves when a hitch lands.

**A ratio law for the target.** A PID assumes a smooth plant. Here the actuator moves
by whole rows, acts a frame or two late, and its measurements carry hitches. The
governor instead scales by target ÷ estimate, as dynamic-resolution controllers do,
with the headroom the Linux `schedutil` governor keeps (it aims 20 % under capacity;
Khora 10 % under the setpoint).

**Not a price signal.** A market would publish one price for a microsecond and let
each agent buy. With discrete rows the price oscillates from round to round, and one
round does not guarantee the frame fits. When agents reveal their whole menu at once,
the market's answer *is* the Q-RAM plan — so the DCC computes it directly, and keeps
the cutoff of its upgrades only for information (`Allocation.price`).

**What it measured.** With `arbiter_eval`, on a journal recorded from the sandbox, the
engine's arbiter decides a round in 26 µs on average (29 µs at most), and gives back
every recorded epoch. The cost book brings the error between quote and measurement
from 0.97 to 0.32. In the simulated scenarios, the engine's arbiter — planning and
cost book — switches rows less often than the greedy and exact search without them:
5.3 against 10.6 switches a minute on the light ramp, 2.4 against 16.0 under heat.
Two tests pin that it does so without running over more often
(`planning_switches_less_without_overrunning_more`,
`the_book_lowers_the_quote_error_without_overrunning_more`).
The exact search's node budget was set so that 10 agents of 4 rows each decide in
under 1 ms in a release build. These are one machine's numbers: run the example to
get your own.

---

## Formalisation

*This part is for readers who want the guarantees stated precisely.*

**Notation.** *A* is the set of agents asked; *D* the devices; agent *a* has weight
*w*<sub>a</sub>, floor *f*<sub>a</sub>, running row *u*<sub>a</sub>, admitted row
*β*<sub>a</sub>, and rows *r* with a quoted cost *ĉ*<sub>a,r</sub> ∈ ℕ<sup>D</sup>
(integer µs), memory *μ*<sub>a,r</sub>, transfer *τ*<sub>a,r</sub> and quality
*q*<sub>a,r</sub>. The waves *W*<sub>1</sub> … *W*<sub>k</sub> and the background
agents *B* are fixed for the round. The target is *T*, the memory known on device *d*
is *M*<sub>d</sub> (unbounded when unknown).

**Predicted cost.** With the book's factor *k*<sub>a,r</sub> and spread
*σ*<sub>a,r</sub>, and *z* = Φ<sup>−1</sup>(1 − ε):

> *c*<sub>a,r,cpu</sub> = (*k*<sub>a,r</sub> + *z* *σ*<sub>a,r</sub> · [*r* ≠ *f*<sub>a</sub>]) · *ĉ*<sub>a,r,cpu</sub>,
> *c*<sub>a,r,d</sub> = *ĉ*<sub>a,r,d</sub> for every other device,

each then × *g*<sub>a</sub> for *r* ∉ {*f*<sub>a</sub>, *β*<sub>a</sub>} when growth
is followed and *g*<sub>a</sub> > 1.

**Critical path** of a plan *x* (one row per agent), on device *d*:

> *C*<sub>d</sub>(*x*) = Σ<sub>j</sub> max<sub>a ∈ W<sub>j</sub> \ B</sub> (*c*<sub>a,x(a),d</sub> + *τ*<sub>a,x(a)</sub>) + Σ<sub>a ∉ ∪W, a ∉ B</sub> (*c*<sub>a,x(a),d</sub> + *τ*<sub>a,x(a)</sub>)

**Planned target.** *T*′ = max(*T* − *m*, max<sub>d</sub> *C*<sub>d</sub>(*β*)), with the
margin *m* = *z* · 1.4826 · MAD(recent critical paths).

**The problem.** Maximise the value

> *V*(*x*) = Σ<sub>a</sub> *w*<sub>a</sub> *q*<sub>a,x(a)</sub> − Σ<sub>a</sub> *π*<sub>a</sub>(*x*(*a*)),
> *π*<sub>a</sub>(*r*) = *w*<sub>a</sub> (*s*<sub>a,r</sub> + *S*<sub>0</sub>) / *H* for *r* ≠ *u*<sub>a</sub>, 0 otherwise,

subject to *C*<sub>d</sub>(*x*) ≤ *T*′ and Σ<sub>a</sub> *μ*<sub>a,x(a),d</sub> ≤
*M*<sub>d</sub> for every *d*, *x*(*a*) in the rows *a*'s rules allow, and
*x*(*a*) = *β*<sub>a</sub> for an agent that keeps its envelope.

**The greedy.** From *x* = *β*, repeat: over every agent and every row *r* allowed with
*q*<sub>a,r</sub> > *q*<sub>a,x(a)</sub> and busiest time no less, take the step of
greatest Δ*V* / Δ*t* (busiest-device µs; +∞ when Δ*t* = 0) with Δ*V* > 0, ties by Δ*V*,
then agent, then row. If *x* with the step breaks a constraint, undo it, rule *r* out
for *a*, and record it as rejected. Stop when no step is left.

**The exact search.** A depth-first search over the agents in name order, each
agent's candidates in decreasing value, starting with best = *V*(greedy). A node is cut
when its value so far plus every remaining agent's best candidate is not above best, or
when every remaining agent sits at its cheapest and the plan already breaks a
constraint. A leaf that is reached fits and beats best, and replaces it. The search
visits at most *N* = 3 072 nodes.

**The share.** With *P*<sub>d</sub> = *C*<sub>d</sub>(*x*<sup>∗</sup>) and
*p*<sub>a,d</sub> the time *x*<sup>∗</sup>(*a*) asks on *d*:

> *b*<sub>a,d</sub> = *p*<sub>a,d</sub> + ⌊ 0.8 (*T*′ − *P*<sub>d</sub>) *p*<sub>a,d</sub> / *P*<sub>d</sub> ⌋,

then bounded by a cap or a `Stable` ceiling (never below the floor), then held: for
each row *r* with *q*<sub>a,r</sub> > *q*<sub>a,x∗(a)</sub> and
*c*<sub>a,r</sub> ≼ *b*<sub>a</sub>, *b*<sub>a,d∗</sub> ← *c*<sub>a,r,d∗</sub> − 1 µs on
the device *d*<sup>∗</sup> where *r* costs most above *x*<sup>∗</sup>(*a*). The host
time is lent as *b*<sub>a,cpu</sub> / *k̃*<sub>a</sub>, *k̃*<sub>a</sub> the planned
factor of *x*<sup>∗</sup>(*a*), and held again at the raw quotes.

**The cost book** for one row, from a run quoting *x* ms and measuring *y* ms, with
λ = 0.95:

> *g* = *P x* / (λ + *x*² *P*), *k* ← *k* + *g* (*y* − *k x*), *P* ← *P* (1 − *g x*) / λ,
> *R* ← λ *R* + (*y*/*x* − *k*)², *n* ← λ *n* + 1, *N* ← *N* + 1,
> *σ* = √( *R*/*n* + *σ*<sub>0</sub>² / (1 + *N*) ),

with *σ*<sub>0</sub> = 0.5. A row unrun for *Δ* runs of its agent is read as
*w k* + (1 − *w*) *k̄*, *w* = 2<sup>−Δ/120</sup>, *k̄* the mean of the agent's other
learnt rows; the spread likewise.

**The governor.** With setpoint *S*, target *τ* = 0.9 *S*, estimate *ê*, multiplier *m*:

> if *ê* > *τ* and a lever exists: *m*′ = max(*m* · max(*τ*/*ê*, 0.7), 0.3);
> if *ê* < 0.85 *τ*: goal = min(*m τ*/*ê*, 1, ceiling − 0.06), *m*′ = *m* + max(0.25 (goal − *m*), 0.02), or goal when less than 0.02 would remain;
> otherwise *m*′ = *m*.

The round shares *S*′ · min(*m*′, safety ceiling), *S*′ being *S* tightened by the
forecast ratio, at least 0.5.

**Propositions.**

- **(A-1) The exact search never lends less.** *V*(exact) ≥ *V*(greedy). *Proof.* The
  best value starts at the greedy's, and is replaced only by a strictly greater one. ∎
- **(A-2) What is lent keeps the rules and fits.** The plan lent satisfies every
  constraint of the problem, or it is the admitted plan *β*, or the round is `Safety`.
  *Proof.* A proposal is weighed only after `Shield::check` accepts it, and the shield
  checks the rules, *C*<sub>d</sub> ≤ *T*′ and the memory on the problem the proposers
  solve. With none accepted, *β* is lent, which admission found to fit *T* (and *T*′ ≥
  *C*(*β*) by construction); when *β* does not fit, the round is `Safety`. ∎
- **(A-3) A round is a pure function of its record.** Costs are integer µs; every
  ordering is total (gain, then name, then row; candidates by a stable sort); the
  corrections, the planning and every other input are recorded. The same record gives
  the same allocation, which is what `evaluation::replay` checks. ∎
- **(A-4) The planned row is the row chosen.** If the agent's quotes hold, it runs
  *x*<sup>∗</sup>(*a*) or a row of equal quality in the same lane that fits its
  envelope. *Proof.* Every row of more quality is held out of the envelope, at the
  agent's own quotes; the planned row fits by construction. ∎ (A row of equal quality
  can still be taken: within one lane the agent prefers the dearer level, "more of the
  same work". The bound of protocol P-a covers it.)

**The cost of a round.** Admission and the share are linear in the agents and devices.
A greedy step scans every agent's rows and checks the plan, so the pass is O(*L*²) in
the worst case, *L* being every menu's rows together — a few dozen. The exact search
visits at most 3 072 nodes, each checking the plan in O(|*A*| · |*D*|).

## Next steps

- [The GORNA protocol](./gorna-protocol.md) — the round the arbiter decides, and
  what an agent does with its envelope.
- [GORNA](./gorna.md) — the overview, the heuristics, the developer's modes.
- [How-to: debug a frame](../how-to/debug-a-frame.md) — read a decision's `Why` in the
  editor.
- [How-to: control GORNA adaptation](../how-to/control-gorna-adaptation.md) — pin,
  bound or record.
