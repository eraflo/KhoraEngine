# The big idea

<p class="kp-lede">Most engines are set up once, by hand, for the machine their makers had in mind, and play the same way everywhere. Khora is built to adjust itself while it runs. Its parts — drawing, sound, physics, scripts — tell a coordinator what each way of doing their work would cost right now. The coordinator shares out the time a frame has. Each part then picks the best way that fits. It happens again and again while the game runs, so the engine follows the machine and the scene as they change.</p>

This page is the *why* of Khora's design. Where each idea lives in the code is
[Architecture (CLAD)](./clad.md); how a frame runs is [The frame](./the-frame.md).

---

## The problem with rigid engines

Most engines decide ahead of time. The render pipeline is fixed, the quality
presets are chosen, the budgets are baked in. That works on the machine the
defaults were tuned for, and worse everywhere else:

| Problem | Impact |
|---|---|
| Static resource allocation | idle hardware, or a bottleneck |
| Manual per-platform tuning | slow, fragile, expensive |
| No awareness of the moment | cannot favour what matters to the player right now |

A game that ships on a desktop, a laptop on battery and a handheld cannot have one
configuration that is right for all three. The usual answer is tuning by hand:
preset after preset, edge case after edge case. The engine never *knows* its
situation; it replays decisions someone made in advance.

## The answer: an engine that negotiates

Khora replaces the fixed order with negotiation. The design has a name: the
**Symbiotic Adaptive Architecture**, [**SAA**](../reference/glossary.md).
*Symbiotic*: the parts neither command nor obey each other. *Adaptive*: adjusting
to the machine is something the engine does all the time, not a setup step.

- A traditional engine is a **tree**: a central loop calls the renderer, the
  physics and the audio in a fixed order, with budgets fixed in advance.
- SAA is a **council**: a coordinator watches, the specialists price their options,
  the coordinator shares out the frame, each specialist chooses. The decisions are
  revisited many times a second.

Three properties follow:

- **Self-optimisation.** The engine measures where the time goes and shares it out
  again: no `MAX_LIGHTS` constant, no preset to pick.
- **Strategic flexibility.** Under load, rendering moves between unlit, lit forward
  and forward+; shadows drop from a 2048-pixel atlas to 1024 or 512; audio mixes
  128, 32 or 8 voices; scripts can serve only their critical behaviours for a
  frame. None of it needs the developer.
- **Goals, not settings.** Every adaptation serves a target the developer sets — a
  frame time, a priority between families — never a fixed quality level.

## The pillars

SAA rests on seven ideas. Here is the idea of each; where each lives is
[Architecture (CLAD)](./clad.md).

### 1. Dynamic Context Core — the coordinator

The [**DCC**](../reference/glossary.md) is the engine's awareness. It commands no
subsystem. It keeps a picture of the whole application: the hardware (processor
load, graphics memory, temperature, battery), what the frames measured, and the
goals (the target frame time). It runs on **its own thread, 20 times a second**,
beside the frame loop and never in it.

### 2. Families and their agents — the specialists

Every subsystem that has choices is a **family**: its **lanes** are the ways it can
do its work, each offered at one or more **levels**, and every level is a **row**
with a price. Each family has an [**agent**](../reference/glossary.md), always the
same one, `LaneAgent`, built by the engine from the family's registration. It
offers the rows, priced on the current frame, and chooses one within the budget it
is lent. The engine ships 11 families: rendering, shadows, the sky, the grid, the
wireframe, the gizmos, the interface, audio, physics, scripting and loading. A
game adds its own the same way. Work with nothing to choose — the asset service,
serialization, ECS maintenance — is a plain service with no agent. See
[Agents and Lanes](./agents-and-lanes.md) and
[Families and levels](./families-and-levels.md).

### 3. GORNA — the negotiation

[**GORNA**](./gorna.md) (Goal-Oriented Resource Negotiation and Allocation) is the
protocol between the DCC and the agents. It has four messages:

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../images/saa/negotiation.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>Offer.</strong> Each agent offers its whole menu: every row, with what it costs this frame on each device and what it is worth.</li>
<li><strong>Allocation.</strong> The DCC shares the frame's time among them and lends each an envelope: time per device, memory. Never a row.</li>
<li><strong>Choice.</strong> Each agent chooses the best row that fits its envelope, and runs it.</li>
<li><strong>Report.</strong> After every run, each reports the row that ran, what it measured, how much of its work it did. The next round decides from what was measured.</li>
</ol>
</figure>

**The DCC decides how much; the agent decides how.** The DCC sets one frame-time
target from its picture of the machine, and a closed-loop **frame governor**
adjusts it until the frames' *measured* time settles under the target.

### 4. Adaptive Game Data Flows — data that rearranges itself

[**AGDF**](./agdf.md) is the idea that not only algorithms but the *layout of data
in memory* should adapt to the hardware. The data layer watches how each component
is read and lays its storage out for the machine it is on:

| Signal | AGDF action (representation only) |
|---|---|
| A component is scanned in tight, vectorisable loops | re-tile its column to a SIMD-friendly layout |
| A component mixes hot and cold fields | split its storage so iteration touches only the hot ones |
| A layout stops paying off on this hardware | repack it, if a cost/benefit test says so |

This moves bytes, never the outcome of the simulation. It is the data layer's
twin of GORNA — the same *observe → decide → apply* loop, applied to memory.

> **What AGDF is *not*.** Removing an entity's physics because it is far from the
> player *changes the game*, not its representation. That is the developer's
> decision. The engine may offer the mechanism; it never applies it to an entity
> the developer has not opted in. See pillar 7.

### 5. Contracts — the common language

For negotiation to work, every part must speak the same language. Khora's
contracts are Rust traits and types in `khora-core`: a family declares its outputs
and whether a cheaper level may change its result (`LaneFamily`, `Semantics`); a
lane declares its levels, what each costs on this frame's inputs, and what it reads
and writes (`Lane`, `Level`, `Cost`, its typed inputs and outputs); a backend
implements a trait (`GraphicsDevice`, `PhysicsProvider`, `AudioDevice`). Those
declarations are what the engine checks at boot, groups into parallel waves, and
negotiates over.

### 6. Observability — the glass box

An adaptive engine must not become a black box. Every GORNA round is recorded with
its inputs and its reasons (`DecisionTrace`), the editor's Control Plane shows them,
and the telemetry service measures every subsystem. A developer can ask not just
"what happened?" but "*why* did the engine choose that?".

### 7. Developer control — partnership, not autocracy

The engine's autonomy serves the developer. Per family, the developer picks an
adaptation mode — `Learning`, `Manual` (pin a row), `Stable` (never upgrade),
`Bounded` (only rows within a range) — and can weigh families against each other.
The boundary is firm:

> **Automatic adaptation may change the *how* (strategy, quality, memory layout)
> but never the *what* (game semantics — which components an entity has, how the
> simulation behaves).** Changing the *what* is always the developer's call; the
> engine supplies the mechanism, the developer writes the policy.

*Adapt the HOW, never the WHAT* recurs throughout the engine. It is why the physics
step is a game setting and never a GORNA lever, why physics and scripting only
offer levels that give the same result, and why adaptation never becomes a
correctness hazard.

## Cold path and hot path

SAA runs on two clocks. The **hot path** is the frame loop, every frame; it never
waits. The **cold path** is the DCC's thread, 20 times a second; it watches what
the frames measured and decides what comes next. One channel joins them:
allocations go down, measurements come up.

<div class="kp-figure-frame">

{{#include ../images/saa/two-clocks.svg}}

</div>

| Aspect | Cold path (DCC) | Hot path (scheduler and agents) |
|---|---|---|
| Thread | its own | the frame loop's, and the scheduler's pools |
| Rate | 20 times a second | every frame |
| Job | observe, decide, share out envelopes | lend envelopes, run the layers, wake agents |
| Communication | publishes allocations on one channel | takes the newest at a frame start; reports after every run |

Agents are *adapters*, not controllers. They receive envelopes and choose the row
that fits; between two choices they are not called at all — the scheduler runs the
layer they installed. The DCC decides how much there is; the agents decide how to
use it. The data layer plays in the same spirit on its own: it adapts its memory
layout inside `khora-data`, and the DCC only observes the result. Only agents take
part in the budget negotiation.

The split is not negotiable. The frame loop is never blocked by analysis, and an
agent never waits on the DCC: an allocation published between two frame starts
replaces one not yet lent, and a frame start lends the newest.

## SAA is the *why*; CLAD is the *how*

The ideas have a concrete form: the [**CLAD**](./clad.md) layering — Control,
Lanes, Agents, Data — and the crates that hold it. Every idea above has one home
there, and the direction of the crates' dependencies *is* the architecture.

## Under the hood

The negotiation reduces to two functions that never meet inside a frame. For each
epoch *e*, the DCC computes, from the offers *O* (each agent's priced rows) and its
picture of the machine *S*, an envelope per agent:

> *share : (O, S) ↦ (E_a)_a*,

aiming to keep the frame's **critical path** — the sum over the frame's waves of the
time of each wave's slowest member, *Σ_w max_{a ∈ w} T_a* — within the frame's
target on every device. An agent at the background phase is on no wave: its time
is not on the path.

Each agent *a* then applies, at a frame start,

> *choose_a : E_a ↦ r_a*, a row of greatest quality among those that fit *E_a*.

The separation is the guarantee: *share* never names a row, so an agent's own
knowledge of its work (which levels exist, what each changes) never has to reach the
DCC; *choose* never sees another agent's envelope, so no agent can take another's
time. The precise rules are in [Families and levels](./families-and-levels.md#under-the-hood)
for *choose* and in [GORNA](./gorna.md) for *share*.

## Next steps

- **[Architecture (CLAD)](./clad.md)** — where each idea lives, and why the layers
  depend on each other the way they do. Read this next.
- **[The frame](./the-frame.md)** — the per-frame mechanics that turn SAA into
  motion.
- **[Glossary](../reference/glossary.md)** — every Khora term in one place.
