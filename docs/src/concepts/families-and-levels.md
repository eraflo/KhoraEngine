# Families and levels

<p class="kp-lede">A family is one job with several ways of doing it. Sound can be mixed with 8 voices, or 32, or 128: the room sounds the same, only richer or poorer. A picture can be drawn flat, or lit, or lit with many lights at once: it hangs in the same frame. Every frame the family is told how much time it has, and it picks the richest way that still fits.</p>

This page explains what a family is made of: its outputs, its lanes, the levels
each lane offers, and the one line that registers it. Then how its levels become
priced **rows**, how its agent chooses one within the envelope the DCC lends, and
how a developer can pin or bound that choice. For how the chosen row runs every
frame, read [Agents and Lanes](./agents-and-lanes.md); for how the envelopes are
decided, [GORNA](./gorna.md).

---

## A family

A **family** is a set of lanes that do the same job and can stand in for each
other. In code it is a unit type implementing `LaneFamily`
(`khora-core/src/lane/contract/family.rs`), which names four things:

| Item | What it says |
|---|---|
| `NAME` | the family's name, as boot errors and telemetry print it |
| `AGENT` | the `AgentId` the family negotiates under; one family per agent |
| `SEMANTICS` | what a cheaper level may change: `Presentation` (it may look or sound worse) or `Exact` (it must give the same result) |
| `Outputs` | what every lane of the family writes: a struct deriving `LaneOutputs` |

The outputs are what makes the lanes interchangeable. The audio family's lane
writes one playback update per source into a deck slot; any other audio lane would
write the same slot, with the same meaning, and the systems that read it would
never know which lane ran:

```rust
pub struct AudioFamily;

impl LaneFamily for AudioFamily {
    const NAME: &'static str = "audio";
    const AGENT: AgentId = khora_core::audio::AUDIO; // AgentId::new("audio")
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = AudioOutputs;
}

#[derive(LaneOutputs)]
pub struct AudioOutputs {
    #[deck]
    pub playback: AudioPlaybackWriteback,
}
```

Each output field says where it goes: `#[pass]` for GPU work recorded for a pass
slot, `#[deck]` for a value a later phase or a Maintenance system reads, `#[staged]`
for what a family running off the frame hands on (see
[Agents and Lanes](./agents-and-lanes.md#how-a-lane-gets-its-data-the-bus-and-the-deck)).

**`Exact` means exact.** Physics, scripting and loading are `Exact`: a cheaper
level may do *less* of the frame's work, never different work. Boot refuses an
`Exact` family whose level changes the result.

## Its lanes and their levels

A **lane** is one way of doing the family's job (`Lane`, with
`type Family = AudioFamily`). A lane listed in another family's registration does
not compile.

A lane offers one or more **levels**: the same work with different parameters.
A level (`Level`, `khora-core/src/lane/contract/level.rs`) is:

| Field | What it is |
|---|---|
| `id` | its `StrategyId` — `LowPower`, `Balanced`, `HighPerformance` or `Custom(n)` — unique within the family |
| `name` | its name for tools and logs ("32 voices") |
| `impact` | how much it changes what the family's readers see: `None`, `Minor`, `Major` |
| `memory` | what it holds on each device, in bytes |

The level's **parameters** stay inside the lane: the lane receives the level's
index and maps it to what it means. The mixing lane mixes at most
`VOICE_CAPS[level]` voices, 8, 32 or 128; the shadows lane draws its shadow
atlas at 512, 1024 or 2048 pixels a side. A lane lists its levels cheapest first.

The impact gives a level its **quality**, from 0 to 1:

| Impact | Quality | Meaning |
|---|---|---|
| `None` | 1.0 | the reference result |
| `Minor` | 0.7 | a difference a player has to look for |
| `Major` | 0.4 | a difference a player sees |

## The menu: rows and their prices

The agent's **menu** is every level of every lane the device supports, lane by
lane, cheapest level first. Each entry is a **row**, named by its level's
`StrategyId`. A lane whose `supports` says the device cannot run it is left off
the menu at boot.

<div class="kp-figure-frame">

{{#include ../images/agents/menu.svg}}

</div>

A row has a **price**: the lane's `cost` for that level on this frame's inputs. A
`Cost` (`khora-core/src/lane/contract/cost.rs`) holds:

- `time`: the time on each device — CPU, GPU, NPU, other (`PerDevice`);
- `transfer`: the bytes sent each way between host and GPU (`PerLink`);
- `memory`: the bytes held on each device;
- `confidence`: from 0 (a guess) to 1 (measured);
- `impact`: the impact on *these* inputs, when it differs from the level's own;
- `share`: the share of the frame's due work the level serves, when it is not
  all of it.

A row's quality is its impact's quality times the share it serves. The script
family shows why the share exists: its two levels, "critical" and "whole", both
have impact `None` — a script that runs gives the same result — but "critical"
serves only the turns of the behaviours marked critical, and its price names that
share.

What the agent offers the DCC is its rows with their prices and qualities, each
tagged `Measured` (the row that ran), `Quoted` (priced on this frame's inputs) or
`Assumed` (not priced yet: its level's memory, no time). It leaves out a row
another lane's row beats on every count — no worse quality, no dearer on any
device — and names its **floor**, the cheapest priced row.

## Choosing a row within the envelope

The DCC answers with an **envelope**: a time per device and a memory per device.
It never names a row. The agent chooses:

1. A row **pinned** by the developer runs, whatever its price.
2. Otherwise, among the rows the developer **allows** that have a price, the rows
   that **fit**: their time and their memory within the envelope on every device.
3. Of those, the best quality. Between equals, each lane's dearest level — more of
   the same work — then the cheapest of those lanes.
4. When no row fits, the **floor** runs, and the agent's report says it is over
   budget.
5. When no row has a price yet, the family's default row runs.

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../images/agents/choose-row.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>The menu, priced.</strong> 40 sources play this frame. The mixing lane prices each level: 4 µs per voice mixed, 0.05 µs per voice past the cap, 2 µs per block. 35.6, 130.4 and 162 µs.</li>
<li><strong>The envelope.</strong> The DCC lends 150 µs on the CPU this epoch. It names no row.</li>
<li><strong>What fits.</strong> 128 voices, at 162 µs, does not fit. 8 and 32 voices do.</li>
<li><strong>The best that fits.</strong> 8 and 32 voices tie at quality 0.7. Within one lane the dearest level wins: 32 voices runs.</li>
<li><strong>Nothing fits.</strong> With 30 µs, no row fits. The floor runs — the cheapest priced row, 8 voices — and the report says over budget.</li>
<li><strong>More room.</strong> With 200 µs, 128 voices fits and is the best, at 1.0. The agent switches; its layer is prepared off the frame.</li>
</ol>
</figure>

A row whose lane failed under the current envelope is passed over until the next
allocation. The prices the agent chooses on are its lanes' latest: every run prices
every level of the lane that ran, and the agent prices all its rows again on its
first frame, when the frame's target changes, when the running row's price drifts
more than 10 % from the last full quote, and at least once a second.

## The registry line

A family is registered by one `register_agent!`, in its folder's
`registration.rs`, submitted to the engine's collection:

```rust
khora_script::inventory::submit! {
    RegisteredAgent(khora_core::register_agent! {
        family: RenderFamily,
        phase: ExecutionPhase::OUTPUT,
        importance: AgentImportance::Critical,
        after: [],
        fixed_step: None,
        default_level: StrategyId::Balanced,
        lanes: [SimpleUnlitLane, LitForwardLane, ForwardPlusLane],
    })
}
```

| Field | What it says |
|---|---|
| `family` | the `LaneFamily`; every lane listed must name it as its `Family`, or the line does not compile |
| `phase` | where the family's place runs: `INIT`, `OBSERVE`, `TRANSFORM`, `MUTATE`, `OUTPUT`, `FINALIZE`, a game's own phase, or `BACKGROUND` (off the frame) |
| `importance` | `Critical`, `Important` or `Optional`; it weighs the family in GORNA, and only `Optional` work is skipped when the frame runs late |
| `after` | the families it runs after within its phase, by `AgentId`; on a frame where one of them did not complete its run, the family is skipped too ([The frame](./the-frame.md#phases-and-waves)) |
| `fixed_step` | `None` to run once a frame, `Some(step)` to run at a fixed step |
| `default_level` | optional: the `StrategyId` the family starts on; else its menu's first row |
| `lanes` | its lanes, each built with `Default` |

There is nothing else to write. The engine builds the family's `LaneAgent` from
this line; the agent's contention follows from its lanes' typed inputs and
outputs; the family starts on, and an app switches it through the `Activation`
resource. Boot checks every registration together, before the first frame, and
refuses the set with a `BootError` naming the family when, among others: two
registrations name one family or one agent; a family offers no level, or one
`StrategyId` twice; no lane runs on the device; an `Exact` family has a level that
changes the result; a lane reads a runtime entry the runtime does not hold, or a
deck slot no earlier phase writes; the default level is not on the menu; a
`BACKGROUND` lane reads anything of a single frame.

## Pins and allowed rows

A developer steers a family through its **adaptation mode** (`AdaptationMode`,
see [Control GORNA adaptation](../how-to/control-gorna-adaptation.md)). Two modes
reach the agent with the envelope:

- `Manual(row)` **pins** the row: it runs whatever the envelope, provided the
  family offers it.
- `Bounded { min, max }` **allows** only the rows whose `StrategyId` lies between
  the two: the choice above runs among them.

`Stable` and `Learning` change how the DCC shares the envelopes, not what the
agent may pick. When a round could not price a family at all, its agent is lent
the pin or the allowed rows without an envelope, and keeps to them: the pinned
row; else the running, default or first row the policy allows that has not failed.

## The families the engine ships

| Family (`AgentId`) | Phase | Importance | Lanes and levels | Semantics |
|---|---|---|---|---|
| `renderer` | `OUTPUT` | Critical | unlit (LowPower) · lit forward (Balanced, default) · forward+ (HighPerformance) | Presentation |
| `shadows` | `OBSERVE` | Important | low-res · medium · standard shadows | Presentation |
| `skybox` | `OUTPUT`, after `renderer` | Important | skybox | Presentation |
| `grid`, `wireframe`, `gizmo` | `OUTPUT`, after `renderer` | Optional | one level each | Presentation |
| `ui` | `OUTPUT` | Important | ui | Presentation |
| `audio` | `TRANSFORM` | Important | 8 · 32 · 128 voices | Presentation |
| `physics` | `TRANSFORM`, fixed step | Critical | standard | Exact |
| `script` | `TRANSFORM` | Critical | critical (LowPower) · whole (Balanced, default) | Exact |
| `loading` | `BACKGROUND` | Optional | rush (HighPerformance, default) · paced (Balanced) | Exact |

Their folders are under `crates/khora-lanes/src/`: `render/`, `shadows/`,
`skybox/`, `overlay/{grid,wireframe,gizmo}/`, `ui/`, `audio/`, `physics/`,
`script/`, `loading/`.

## Under the hood

For contributors: the choice is `choose` in `khora-agents/src/lane_agent/choice.rs`,
the offer `LaneAgent::offer` in `lane_agent/mod.rs`, the registration and its
checks `khora-core/src/agent/registration.rs`.

**Notation.** A menu is rows *1 … n*. Row *r* has a price *c(r)* — a time
*t_d(r)* and a memory *m_d(r)* for each device *d* — or none, and a quality
*q(r) = q(impact(r)) · s(r)*, with *s(r)* ∈ [0, 1] its share. An envelope *E* is a
time *T_d* and a memory *M_d* per device. Let *A* be the rows the policy allows
(all of them without a `Bounded` mode) and *P ⊆ A* the priced ones; a row that
failed under the current envelope counts as unpriced. A row fits
when

> *fits(r, E) ⇔ ∀d : t_d(r) ≤ T_d ∧ m_d(r) ≤ M_d*,

and the feasible set is *F(E) = { r ∈ P : fits(r, E) }*.

**The choice.** Without a pin, *choose(E)* is a row of *F(E)* of greatest
quality when *F(E) ≠ ∅*; otherwise the floor *argmin_{r ∈ P} max_d t_d(r)* when
*P ≠ ∅*; otherwise the default row.

**Theorem 1 (the choice fits whenever something does).** If *F(E) ≠ ∅*, the row
chosen fits *E* and no fitting row has a greater quality. *By definition* of the
first case; the tie-breaks only choose among rows of equal, greatest quality.

**Theorem 2 (more room never lowers the quality).** If *E ≤ E′* on every device,
in time and memory, then *q(choose(E)) ≤ q(choose(E′))* whenever *F(E) ≠ ∅*.
*Proof.* Every row that fits *E* fits *E′*, so *F(E) ⊆ F(E′)*, and the greatest
quality over a larger set is no smaller. The row may change between equals; its
quality cannot fall.

**Theorem 3 (over budget means the floor itself does not fit).** The agent reports
over budget exactly when *P ≠ ∅* and *F(E) = ∅*; then the row that runs is the
cheapest priced one, so no allowed row could have run within the envelope.

**Bounds.** The menu has *Σ_lanes |levels|* rows, at most one per `StrategyId`
(boot refuses a duplicate). The choice reads each row once: it is *O(n)* in the
rows, and *n* is 3 for every family the engine ships today. A row's quality lies in
[0, 1]; the share is clamped there (`Cost::served`), a share that is not a number
counting as 0.

**Prior art.** Picking the best quality that fits a budget, from a menu priced on
the current inputs, is what adaptive video streaming does with its bitrate ladder,
and what dynamic-resolution renderers do per frame. Here every subsystem has such
a ladder, and the budget comes from a negotiation rather than a fixed target.

## Next steps

- [Agents and Lanes](./agents-and-lanes.md) — how the chosen row runs every frame,
  as a layer.
- [GORNA](./gorna.md) — how the envelopes are shared out.
- [How-to: add a subsystem (a family)](../how-to/add-a-family.md) and
  [add a lane](../how-to/add-a-lane.md).
- [Control GORNA adaptation](../how-to/control-gorna-adaptation.md) — pin or bound
  a family's row.
