# Agents and Lanes

<p class="kp-lede">Think of a kitchen at dinner time. Each station has one dish to send out, and several recipes for it: a quick one, a careful one, an elaborate one. The head of the kitchen tells each station how much time it has tonight. The station's chef picks the best recipe that fits that time, then the cooks follow it, plate after plate, without asking again. The chef is called back only when something changes: less time, a missing ingredient, a dish that failed.</p>

In Khora the station is a **family**, the recipes are its **lanes**, the chef is
its **agent**, and the head of the kitchen is the [DCC](./gorna.md). This page
explains the two roles, how a chosen lane runs every frame without its agent, and
how a lane reaches the data it works on. For the families themselves, their levels
and how a row is chosen, read [Families and levels](./families-and-levels.md). To
write one, see [Add a lane](../how-to/add-a-lane.md) and
[Add a subsystem (a family)](../how-to/add-a-family.md).

---

## Two roles, one descent

A frame's work goes down four layers, written `Control → Agent → Lane → Data`
and named **CLAD** after them (see [Architecture (CLAD)](./clad.md)). Control
lends each agent an **envelope**: how much time it may spend on each device, and
how much memory it may hold. The agent chooses a way to do its work within it.
The way it chose does the work, on data the Data layer prepared for it.

The two middle roles are different kinds of object:

- A **lane** is a worker. It does one job — draw the shadow maps, step the
  physics, mix the sound, lay out the interface — and decides nothing. It offers
  **levels**: the same job with different parameters, at different costs (8, 32
  or 128 voices of sound). It prices each level on the frame's inputs, and records
  at the level it is given.
- A **family** groups the lanes that do the same job. They write the same outputs
  and run at the same place in the frame, so any of them can stand in for another.
  The render family has three lanes: unlit, lit forward and forward+.
- An **agent** is the family's strategist. Every family has the same one,
  `LaneAgent`, built by the engine from the family's registration. Its **menu**
  is every level of every lane the device supports, each one a **row**. It offers
  the menu to the DCC, receives an envelope, and chooses the row that fits.

**The agent owns the choice. The lane owns the work.** A new subsystem is a new
family — its lanes and one registration line — and never a new agent type:
`LaneAgent` is the only implementation of the `Agent` trait outside tests, and an
xtask check (`only_lane_agent_implements_agent`) keeps it so.

## Why agents are kept so thin

An agent implements **only** the `Agent` trait: no `start` or `stop`, no
builders, no accessors. The trait has no `execute` either: an agent never does a
frame's work. Three reasons:

- **One family per agent.** An agent has exactly one thing to negotiate. Shadows
  are a family of their own, not a job of the renderer: the shadow maps cost
  something else, run in another phase (`OBSERVE`, before the scene draws in
  `OUTPUT`), and can be cheapened on their own.
- **No per-frame state.** An agent holds its strategy — its lanes, its quotes,
  the row it chose, the envelope it was lent — and nothing that a frame
  produces. A subsystem's own state lives in the runtime (`Runtime::services`,
  `Runtime::backends`), where the physics provider and the scripting runtime
  are; what a run measured lives at the family's place (`LastRun`).
- **Strategist, never worker or controller.** An agent contains no pipeline code
  (that is the lane's) and decides no global priority (that is the DCC's).

A subsystem with *nothing to negotiate* is not a family at all. ECS maintenance,
the asset service and serialization are **services** (`EcsMaintenance`,
`AssetService`, `SerializationService`): there is nothing for GORNA to choose
between.

## Lanes are layers

Picture the frame as a sheet of paper with transparent sheets laid on it, each at
its own height. An agent chooses a sheet — a lane at one level — and lays it at its
family's height. To change, it swaps the old sheet for a new one at the same height.
The frame itself just applies the sheets that lie there, frame after frame.

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../images/agents/layers.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>A place for each family.</strong> Its <em>place</em> is fixed: its phase in the frame and, for what draws, its pass. On it lies one <em>layer</em>: one of its lanes, prepared at one level.</li>
<li><strong>The scheduler applies the layers.</strong> Every frame it runs each installed layer at its place, without calling or locking any agent. On a quiet frame an agent costs nothing.</li>
<li><strong>The layer wakes its agent.</strong> Each run prices itself. When the price no longer fits the envelope — or an allocation arrives, the lane fails, an input is refused — the layer leaves a <em>wake</em> for its agent.</li>
<li><strong>A new layer, prepared beside.</strong> Woken once the frame's Views are published, the agent chooses the row that fits. A fresh layer of it — its pipelines, its buffers — is prepared off the frame, while the old layer keeps running.</li>
<li><strong>The swap, at a frame start.</strong> At the first frame start after it is ready, the new layer takes the old one's place: the same place, the same outputs. No frame is ever left without a layer.</li>
<li><strong>Put aside, then released.</strong> The old layer is kept ready for three allocations, so going back to it prepares nothing; then it is released — at once if the envelope's memory cannot hold both.</li>
</ol>
</figure>

The family's `Place` is shared by its agent and the scheduler. It holds the
layer installed, the one staged for the next frame start, the ones put aside
with the allocations they have left (`KEEP_EPOCHS`, 3), the wakes gathered for
the agent, and the **terms** of the run: the epoch, the envelope, the time lent to
a lane that reads a budget, the fixed step. Its logic is written once, in
`khora-core`; the scheduler and the tests run the same code.

New terms reach the installed layer at its next run, even while another layer is
on its way in. They come in two parts. The **agent's part** — the epoch, the
envelope, the budget, the fixed step, whether the row runs over budget, when the
agent last quoted, the frame's target — applies to the installed layer at once, so
its reports answer the epoch just lent. The **row's part** — the quote the row's
drift is measured from — belongs to one row, so a new row's quote applies only from
that row's swap.

A frame goes like this:

1. **Lending.** At the frame start, the allocation in force is lent to the agents
   that do not hold it yet. Each keeps it and leaves itself a wake.
2. **Swap.** Every place swaps in the layer staged for it.
3. **Event pass.** Once the Flows have published the frame's Views, every agent
   whose place holds a wake is woken, once. It takes in what its layer's last run
   found, prices its rows again when the prices may no longer hold, and chooses.
   A row it ran recently is reinstated from the layers put aside; a new one is
   left to prepare off the frame. Into an empty place — the first frame — the
   layer is prepared at once.
4. **Waves.** The layers installed run, phase by phase, grouped into waves by what
   they touch (below). Each run reports.

The agent is woken for:

| Wake | What happened |
|---|---|
| `FirstFrame` | nothing is installed yet |
| `Allocated` | a new envelope, pin or set of allowed rows was lent |
| `QuoteUnfit` | the running row's price stopped fitting the envelope |
| `QuoteDrift` | the running row's price moved more than 10 % (`QUOTE_DRIFT`) from the last full quote |
| `QuoteAge` | the last full quote is older than 1 s (`QUOTE_MAX_AGE`) |
| `TargetChanged` | the frame draws into a target of other formats |
| `LaneFailed` | the lane returned an error |
| `InputRefused(reason)` | an input was refused, and why |
| `PrepareFailed(row)` | the new layer of that row could not be prepared off the frame |

`FirstFrame`, `QuoteDrift`, `QuoteAge` and `TargetChanged` make the agent price
every row again. A failed row is passed over until the next allocation, and the
agent falls back to a cheaper one. The first report after a swap says how long the
new layer took to prepare (`Report.switch`): what a change of row costs, measured.
A panic in a layer's run, in a wake or at a frame start fails that family alone,
for that frame; the wakes it was handed are given back, so the next frame wakes
the agent again.

## How a lane gets its data: the bus and the deck

A lane never reaches into the ECS `World`. It reads and writes through two typed
channels that live for one frame, reached only through its typed inputs
(`#[derive(LaneInputs)]`) and its family's typed outputs (`#[derive(LaneOutputs)]`):

- **The `LaneBus`, the read side.** During the Substrate Pass each **Flow** reads
  the World and publishes one typed **View** — `RenderWorld`, `AudioView`,
  `ShadowView`, `UiScene`… — on the bus. Flows are the only producers; a lane
  only reads, and the bus is shared read-only once the agents run.
- **The `OutputDeck`, the write side.** A lane writes its family's outputs into
  typed **slots**: a recorded pass for a pass slot, a value a later phase reads,
  a write-back for the World. The engine takes them out at the I/O boundary.

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../images/agents/bus-and-deck.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>The Flows publish.</strong> During the Substrate Pass each Flow reads the World and publishes one typed View on the <code>LaneBus</code>.</li>
<li><strong>The layer is in place.</strong> The DCC lent the agent an envelope; the agent installed a layer, a lane at the level that fits. It is not called on this frame.</li>
<li><strong>The lane reads.</strong> Only its typed inputs: Views from the bus, a slot an earlier phase wrote, a runtime entry, the frame's target. Never the World.</li>
<li><strong>The lane writes.</strong> Its family's outputs, committed to the <code>OutputDeck</code> once its record returns.</li>
<li><strong>The I/O boundary.</strong> The engine takes the slots out: it submits the recorded passes in their order, and the Maintenance systems write results back into the World.</li>
</ol>
</figure>

The whole path is **World → Flow → LaneBus → Lane → OutputDeck → I/O boundary**,
with the agent above the lane choosing *which* row runs and the DCC above the
agent setting *how much* it may spend. Nothing crosses between two lanes but a
typed value. The `LitForwardLane` does not know a `ShadowsLane`: it reads the
`ShadowFrame` the shadows family wrote in the `OBSERVE` phase. The shadow lane can
be rewritten, and as long as it writes the same type, the lit lane never notices.

A family's outputs go to one of three places, by the attribute on the field:

| Output | Goes to | Taken by |
|---|---|---|
| `#[pass]` | a pass slot, a `Pass<'f, S>` recorded on the GPU | the engine, which submits the slots in the order `register_pass!` gives them |
| `#[deck]` | a deck slot, an owned value | a later phase's `#[deck]` input, or a Maintenance `DataSystem` |
| `#[staged]` | the reserve, for a run off the frame | the `Boundary` systems of the first frame after the run ends |

## Events and requests: the channels

The bus and the deck last one frame. What happens at one moment and is read at
another — a key pressed, two bodies touching, a file saved, a request to load an
asset — goes into a **channel**, a numbered queue that someone fills and others
read later, like a ticket roll at a counter.

There is one `Channel<T>` per kind of event, each its own runtime entry, never one
shared bus: a reader declares only the events it reads, and contends with no one
else. Each reader keeps its own place in the queue, a **cursor**, so reading
changes nothing in the channel: many readers read at once, and a lane that reads
events declares a read, not a lock. A reader with nowhere to keep a cursor, such as
a data system, reads under a name (`read_for("name")`). A `#[take]` field is
different: it **drains** the channel — its family's mailbox of requests — so the
requests are the lane's alone, and it counts as a lock.

A channel is always **bounded**, and there is no default size: whoever makes one
chooses what a full channel does.

| When full | For | Channels |
|---|---|---|
| `DropOldest`: the oldest item leaves | streams where the recent matters more than the complete | input (2 048), collisions (4 096), script events (4 096), asset changes (4 096) |
| `Reject`: the new item is refused | requests a consumer must answer in order | asset requests (4 096), the project index (64), scene loads (64), script compiles (1 024) |

Nothing is lost silently. What a full channel lost is counted by `dropped`; what a
reader skipped or threw away on purpose, by `discarded`. A reader left behind by a
drop resumes at the oldest item still held: it loses history, never its place. An
item type may declare that a newer item **supersedes** an earlier one still queued
(`Supersedes`): two saves of one file between frames are one reload, while two
collisions stay two. And when a lane's inputs are refused after its `#[take]` has
drained its requests, they are given back to the front of their channel
(`Restore`): the lane never saw them, so the next run does.

## What an agent declares

Two families can run side by side only if they do not touch the same thing. What
a family touches is its **contention** (`Contention`): the deck slots it writes
and reads, and the runtime entries — resources, services, backends — it reads or
**locks**. A lock is what "writing" a runtime entry means: an entry is reached as
`&T`, so it changes behind a `Mutex`, and two families holding one mutex at once
would wait for each other.

Nobody writes a contention by hand. Each lane's typed inputs and outputs are its
declaration, and the contention follows from it:

| Lane field | Contention |
|---|---|
| `#[bus]` | nothing: the bus is read-only for everyone |
| `#[resource]`, `#[resident]` | reads the entry |
| `#[locked]`, `#[take]` | locks the entry (draining a channel empties it for any other reader) |
| `#[deck]` input | reads the deck slot |
| `#[frame]` target or clear colour | reads the frame context |
| `#[pass]`, `#[deck]` output | writes the slot |
| `#[staged]` output, `#[budget]` | nothing on the frame's deck or runtime |

A run contends as the layer installed. When the agent is woken, it contends as
its chosen lane, plus what its other lanes read and lock (it prices them on their
inputs), plus the graphics device and pipeline system it prepares layers with.

Two families share a concurrent **wave** only when their contentions are
disjoint and neither comes `after` the other. Reads never conflict with reads; a
writer and a reader of one deck slot never share a wave, so the reader sees what
the earlier wave wrote.

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../images/agents/waves.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>One phase, in order.</strong> The scheduler sorts a phase's families so each comes after the ones its registration names in <code>after</code>. Each has its contention.</li>
<li><strong>Reads share.</strong> A opens the first wave. B joins it: two reads of R never conflict, and they write different slots.</li>
<li><strong>A slot read waits for its writer.</strong> C reads S1, which A writes: it opens the second wave, and reads S1 as A left it.</li>
<li><strong>An order is kept.</strong> D runs after C, so it never shares C's wave: it opens the third.</li>
<li><strong>The waves run.</strong> A wave of one runs on the scheduler's thread. A larger one runs on the worker pool, each member into its own share of the deck, folded back in wave order.</li>
</ol>
</figure>

The declaration is enforced. An agent or a lane reaches a runtime entry through
`EngineContext::resource` (to read) or `EngineContext::locked` (to lock), and
both refuse, with an error naming the agent and the type, what the contention
does not name. A lane cannot even ask: its inputs are fetched for it. The deck
slots a run writes are checked in debug builds: each one the contention does not
name is logged as an error and listed by
`ExecutionScheduler::undeclared_deck_writes`. A release build does not check.

**Declaring more than you touch is not harmless.** A lane that reads an entry it
never uses still keeps another family out of its wave. Nothing turns red; the
frame is just slower. Read what you use, lock only what you change.

## The life of a lane

A lane answers a few questions, asked by its agent and its layer:

- **`levels`**: its levels, cheapest first. Each is a row of the menu.
- **`supports`**: whether the device can run it at all. A lane that cannot is
  left off the menu at boot.
- **`prepare`**: build what a level needs on the device — pipelines, buffers —
  for the frame's target. Called for a new layer, off the frame, and again by a
  run when the level or the target's formats change.
- **`cost`**: what a level costs on this frame's inputs, per device. Every run
  prices every level of its lane, so the agent's prices stay fresh.
- **`record`**: do one frame's work at one level, and write the family's
  outputs. It returns a `LaneReport`: how much of the work was done, whether the
  lane could not progress, and an optional message.
- **`shutdown`**: release what `prepare` built, when a layer is released or the
  engine stops.

A lane records through `&mut self`, so it may keep buffers from frame to frame;
everything else it reads arrives through its inputs for that frame. An error is a
`LaneError`: it wakes the agent, which falls back to a cheaper row. A lane error
never stops the frame.

## Why this separation exists

1. **GORNA has something to negotiate.** A family's levels are its rows, its agent
   offers them, prices go up to the DCC and envelopes come back down. A single
   "render system" with no alternatives would have nothing to offer.
2. **The hot path never waits for the cold path.** Envelopes are decided on the
   DCC's own thread and lent at a frame start; layers run on Views the Flows
   already published; agents are woken only by events, and new layers are
   prepared off the frame.
3. **Subsystems stay swappable.** Lanes meet only through the bus and the deck,
   so a lane can be added, removed or replaced alone — which is what lets the
   engine move from lit forward to forward+ in the middle of a session when the
   lights pile up.

## Under the hood

For contributors: the contract is in `khora-core/src/lane/` (`contract/` for
`Lane`, `LaneFamily`, `Level`, `Cost` and the I/O derives' traits, `plumbing/` for
the bus, the deck and the reserve), the `Agent` trait and the `Place` in
`khora-core/src/agent/`, the `LaneAgent` in `khora-agents/src/lane_agent/`, and
the waves in `khora-control/src/scheduler/waves.rs` and `execution.rs`.

**Two locks, one order.** An agent and its place each sit behind a mutex. Whatever
holds both — the DCC asking for an offer, the scheduler lending an allocation, the
event pass waking the agent — takes the agent's lock first, then the place's. A
layer's run, a swap and a run off the frame take the place's alone. With one order
and no lock taken the other way, no two threads can each hold one lock and wait for
the other.

**Lock waits share one deadline.** A lane's `#[locked]` inputs are taken when its
inputs are fetched (`InputSource::lock`, `khora-core/src/lane/contract/io/`). Every
lock of one fetch shares one deadline, started at its first wait: the lane's
budget, or `LOCK_WAIT` (2 ms) without a positive one, at most `MAX_LOCK_WAIT`
(250 ms). A lock still held elsewhere at the deadline — or held by the same fetch —
is refused as `LockTimedOut` rather than blocking a worker thread: the run records
nothing that frame, and its agent is woken (`InputRefused`) once, not on every
frame the refusal lasts. A quote does not wait at all: a lock
held elsewhere refuses it at once.

**Notation.** For a family *f* with its installed layer, write *W(f)* for the
deck slots it writes, *R(f)* for the slots it reads, *Rd(f)* for the runtime
entries it reads and *L(f)* for those it locks. Two families **conflict** when

> *W(f) ∩ W(g) ≠ ∅*, or *W(f) ∩ R(g) ≠ ∅*, or *R(f) ∩ W(g) ≠ ∅*, or
> *L(f) ∩ (L(g) ∪ Rd(g)) ≠ ∅*, or *(L(f) ∪ Rd(f)) ∩ L(g) ≠ ∅*.

Reads against reads are absent from the list: they never conflict.

**The partition.** A phase's families, sorted *f₁ … fₙ* so that every `after` edge
points backwards, are cut into waves greedily: *fᵢ* joins the current wave when
it conflicts with no member and depends on none; otherwise it opens a new wave.

**Invariant 1 (a wave is safe).** No two members of one wave conflict, and no
member depends on another. *By construction*: a family joins a wave only after
being checked against every member. The scheduler checks it again on every wave
in debug builds.

**Invariant 2 (order is kept).** If *g* comes `after` *f*, *g* runs in a later
wave than *f*. *Proof.* The sort puts *f* before *g*. When *g* is reached, either
*f* is in the current wave, and *g* opens a new one, or *f* is in an earlier wave
already. Waves run in order.

**Theorem (determinism).** The deck after a phase is the same whatever order the
members of each wave finish in. *Proof.* Each member of a wave writes into its own
shard, which starts with only the slots it declares. Its writes are disjoint from
every other member's (Invariant 1), and no member reads a slot another writes, so
no member can observe another. The shards are folded back in wave order, not
finishing order, and their slot sets are disjoint, so the fold is a union that
does not depend on order. The next wave reads the folded deck.

**The layer invariant.** After its first frame, a place has exactly one installed
layer at every frame start, and the installed layer changes only at a frame
start. So within a frame every family runs exactly one row, and a family's outputs
always come from one lane. A layer put aside is shut down after `KEEP_EPOCHS` = 3
allocations. **Bound:** at the end of every frame start with an envelope lent,
either no layer is put aside, or the memory of the installed layer and the layers
put aside, summed per device, is within the envelope's memory on every device —
the oldest are shut down first until it is. Keeping a way back never costs memory
the DCC did not lend.

**Prior art.** The split between choosing and doing is a strategy pattern with
the choice made by a negotiator rather than a caller. Grouping systems into
parallel batches from declared reads and writes is what Bevy's scheduler and
Unity DOTS' job dependencies do; here the declarations are derived from typed
fields rather than written beside the code.

## Next steps

- [Families and levels](./families-and-levels.md) — what a family is, its levels
  and rows, and how an agent chooses within its envelope.
- [GORNA](./gorna.md) — how envelopes are decided.
- [The frame](./the-frame.md) — where layers, waves and the background fit in a
  frame.
- [How-to: add a lane](../how-to/add-a-lane.md) and
  [add a subsystem (a family)](../how-to/add-a-family.md).
- [Glossary](../reference/glossary.md) — Agent, Lane, Layer and place, LaneBus,
  OutputDeck, Flow, View.
