# The frame

<p class="kp-lede">A game is a flipbook: it draws one picture, then the next, sixty or more times a second, and the motion is in the difference between them. Each picture is called a frame. Khora makes every frame the same way, in the same order of steps — read what the player pressed, move the world, draw it, tidy up — and a second, slower clock beside it decides how much each part of the work may spend.</p>

This page follows one frame from start to end: the stages it walks through, the
boundary where it starts, the phases and waves in which the families run, the work
that runs beside the frame, and the fixed clock that keeps the simulation the same
whatever the frame rate. For the layering it brings to life, see
[Architecture (CLAD)](./clad.md).

---

## Two clocks, one channel

Khora has two clocks. The **hot path** is the frame loop, on the main thread, every
frame; it never waits. The **cold path** is the DCC's own thread, which ticks 20
times a second (`DccConfig::tick_rate`), reads what the frames measured, and opens
a [GORNA](./gorna.md) round when one is due. They meet in one channel: allocations
go down, lent at the next frame start; reports and telemetry come back up.

<div class="kp-figure-frame">

{{#include ../images/saa/two-clocks.svg}}

</div>

A frame never sees a decision in its middle: an allocation is lent only at a frame
start, and of two published between frame starts, the later one wins. The rest of
this page is the hot path. The cold path is the subject of
[The big idea](./saa.md) and [GORNA](./gorna.md).

## Startup, once

Before the first frame, the engine boots through the SDK entry point. It builds
the window hidden, constructs your app, and runs your bootstrap closure. In that
closure the app installs what it needs from the engine: the renderer, the
pipeline system, physics, layout, audio. What it leaves out, the boot fills with the
engine's own: Rapier for physics, Parley and swash for text, a 48 kHz mix bus for
sound (see [what the engine installs](../reference/sdk.md#what-the-engine-installs-when-the-app-brings-none)).
Then the engine registers its services
and the DCC, calls `setup` once so you can spawn initial entities, then `provide`
so you can install runtime entries, and builds one agent per registered family.
The first frame is painted, then the window is shown. Nothing in `setup` runs
again; the engine enters the frame loop and stays there.

The engine runs first; the slow work comes after, as ordinary frames. An app does
not index a project, load a scene or compile scripts inside `setup`. It files each
of them and keeps the ticket it gets back:

- the project index goes to the `IndexHub`;
- a scene goes to the `SceneLoadHub`;
- scripts go through `scripts::mount`, which returns a ticket.

The `loading` family does that work off the frame (see
[The background](#the-background) below), within the budget the DCC lends it, and
each result lands at a later frame's boundary.

An app that must know when its startup is over waits on those tickets through the
runtime's `Readiness`. It can also take a `hold` while it files work in steps. The
engine calls the app's `startup_ready` once, on the frame where everything has
landed, or after 60 s, with the late tickets reported as problems. The editor's
loading film is painted by those first frames and fades on `startup_ready`.
`run_default` loads a game's default scene and scripts the same way, with no film.

One exception keeps `setup` simple for a game built in code: scripts mounted in
the bootstrap closure, before the boot, are compiled by the boot. Their
declarations are then registered before `setup`, which can attach a behaviour by
name. The sandbox works this way.

## The six-stage descent

Each frame runs six stages in order. Each is a public method of the engine core,
so a driver — the winit runner, the editor's shell — can call the app's hooks
between them:

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../images/frame/stages.svg}}

</div>
<ol class="kp-steps-text">
<li><strong><code>drain_inputs</code>.</strong> The frame's clock opens, telemetry ticks, and the input events queued since the last frame are read (not taken: other readers see the same events). Then the app's <code>before_frame</code>: the place to switch families on or off for this frame.</li>
<li><strong><code>run_app_update</code>.</strong> The frame boundary (below), then the data systems around the app's <code>update</code>: <code>PreSimulation</code> before it, <code>PostSimulation</code> and <code>PreExtract</code> after it.</li>
<li><strong><code>begin_render_frame</code>.</strong> The renderer acquires the frame's image and publishes its target and clear colour. Then the app's <code>before_agents</code>: the place to point the renderer at an offscreen viewport.</li>
<li><strong><code>run_scheduler</code>.</strong> The clocks advance, the Flows publish the frame's Views, the renderer is prepared from the camera, the agents with an event are woken, and the phases run their layers in waves.</li>
<li><strong><code>end_render_frame</code>.</strong> The recorded passes are submitted in their registered order; the driver runs the app's <code>after_agents</code> (an overlay drawn there paints over the scene); the image is presented.</li>
<li><strong><code>run_maintenance</code>.</strong> The <code>Maintenance</code> data systems drain the deck into the World — physics and audio write-backs — and compact storage. Then <code>close_frame</code> reports the frame's busy time to the DCC.</li>
</ol>
</figure>

Two stages show the architecture's discipline.

**`run_app_update` does not project the scene.** It keeps the World consistent
around your game logic: input-driven changes the update must see before it,
hierarchy fix-ups such as transform propagation after it, then GPU uploads such as
the meshes'. Projecting the World into Views happens later, in `run_scheduler`.

**`run_maintenance` is the data layer's own housekeeping.** It is not negotiated
with the DCC: it writes back what the lanes left in the deck and compacts storage,
on the data layer's own terms. Lanes feed two sinks: recorded GPU passes,
submitted at stage 5, and typed deck slots, drained at stage 6. Their inputs are
the Views the Flows publish at stage 4.

## The frame boundary

The frame starts at one instant, the **boundary**, at the top of
`run_app_update`. Everything that changes *between* frames changes there, so the
whole frame sees one state:

1. **The families settle.** The requests to switch families on or off, made since
   the last boundary, take effect (`Activation::settle`); mail left for a family
   now off is dropped when its mailbox says so.
2. **The scheduler's frame start.** The runs off the frame that ended come home,
   with what they staged. The allocation in force is lent to every agent that
   does not hold it yet. Every family's place swaps in the layer staged for it.
3. **The `Boundary` data systems.** What work off the frame staged — an indexed
   project, a decoded asset, a prepared scene — lands in the World here, before
   anything of the frame reads it.
4. **Startup.** On the first frame that finds every awaited ticket landed, the app's
   `startup_ready` is called, once.

A request made later in the frame counts from the next boundary.

## Which families run

The **activation** (`Arc<Activation>`, a resource the SDK publishes before the
app's `setup`, `khora_core::agent::Activation`) knows every registered family, and
every family starts on. `activate(family)` and `deactivate(family)` are requests,
settled at the next boundary; a family it does not know is refused
(`UnknownFamily`) and logged.

A family off is not asked to offer, is lent nothing, and does not run. The data
systems and Flows that do its work are skipped too, by their **role**: each one
declares `Role::Upkeep` (a data invariant, run every frame) or
`Role::Serves(family)` (skipped while the family is off). On its first run after a
skip, a system sees `SystemContext.resumed`, and a Flow `FlowContext.resumed`.

The editor switches families in `before_frame`
(`khora-editor/src/commands/transport.rs`). Editing has scripting
(`khora_core::script::SCRIPT`), physics (`khora_core::physics::PHYSICS`) and audio
(`khora_core::audio::AUDIO`) off: nothing steps because nothing that steps runs.
Play switches the three on. A game without an editor keeps every family on unless
it switches one off.

## Phases and waves

`run_scheduler` runs the frame's families in **phases**, in this order:

| Phase | For |
|---|---|
| `INIT` | setting up the frame |
| `OBSERVE` | reading and extracting: the shadow maps are drawn here |
| `TRANSFORM` | simulation and logic: physics, audio, scripts |
| `MUTATE` | syncing results |
| `OUTPUT` | what leaves the engine: the scene, the sky, the interface, the overlays |
| `FINALIZE` | end-of-frame work |

A game may add phases of its own (`ExecutionPhase::custom`, ids 6 to 254) through
its `PhaseProvider`: they run after `OUTPUT` and before `FINALIZE`, in the order it
declares them. Within a phase, each family's installed **layer** runs at its place:

- **In order.** The phase's families are sorted so each runs after the ones its
  registration names in `after` (the sky and the overlays after the renderer);
  where `after` says nothing, the more important run first (`Critical`,
  `Important`, then `Optional`).
- **Only after what they follow.** A family runs only if every family it names in
  `after` has completed its run earlier in the same frame. If one is off, was
  skipped or failed, the family is skipped for the frame too, and so, in turn, are
  the families after it.
- **Checked at boot.** A family may name in `after` a family of its own phase or of
  an earlier one, in the order the scheduler runs, a game's own phases included.
  The fixed-step rounds run before the other families, so a once-per-frame family
  may also name a fixed-step family of any phase; a fixed-step family may name
  only fixed-step ones. Boot refuses, with a `BootError` naming the family and the
  one it names, anything that could never have run first: a family not registered,
  the family itself, one of a later phase, a once-per-frame family named by a
  fixed-step one, one at the background, an `after` on a family at the background
  (nothing orders a run off the frame), and families naming one another in a cycle.
- **In waves.** Consecutive families whose contentions do not overlap, and that do
  not depend on each other, run side by side: a wave of one on the scheduler's
  thread, a larger one on the scheduler's worker pool. See
  [Agents and Lanes](./agents-and-lanes.md#what-an-agent-declares).
- **Fixed steps first.** A family whose place holds a fixed step (physics) runs
  its sub-steps first, round by round, each a full run of its layer; then the
  phases run once more for every other family.
- **Optional work gives way.** A family registered `Optional` (the grid, the
  wireframe, the gizmos) is skipped for the frame when, by its turn, the
  scheduler's frame has already run for more than 16 ms. `Critical` and
  `Important` work is never skipped for time.

No agent is called in this loop. Before the first phase, the **event pass** has
woken each agent whose place gathered a wake — a new envelope, a price that stopped
fitting, a failed lane — so it could choose a row and, if it changed, leave a new
layer to prepare. On a quiet frame no agent is called at all.

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../images/frame/run.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>The frame start.</strong> The allocation in force is lent, each place swaps in its staged layer, and the runs off the frame that ended come home.</li>
<li><strong>The boundary.</strong> What a run off the frame staged lands in the World, before anything of the frame reads it. Then the app's update, between the data systems.</li>
<li><strong>Views, then wakes.</strong> The Flows publish the frame's Views; every agent with a wake is woken once, and the layer of a new row is prepared off the frame.</li>
<li><strong>The phases.</strong> In order, each family's installed layer at its place, in waves; fixed-step sub-steps first.</li>
<li><strong>The background.</strong> Beside the frame, on its own pool: the families placed at <code>BACKGROUND</code> run over several frames, and new layers are prepared.</li>
</ol>
</figure>

## The background

Some work does not fit in a frame: indexing a project, decoding a large asset,
preparing a scene, compiling scripts, building a new layer's pipelines. It runs
**beside** the frame, on a pool of threads of its own, and lands at a boundary.

A family registered in the `BACKGROUND` phase (the `loading` family) is in no phase
order. It runs when its mailbox (a `#[take]` input) holds a request: one run at a
time, possibly over several frames. Nothing waits for it. Its lanes may read only `#[take]`, `#[resource]` and
`#[budget]` inputs, and hand on only `#[staged]` outputs: boot refuses anything of a
single frame. For the run, its layer is **checked out** of its place: the place is
never held across frames, and while the layer is out the place runs nothing and
swaps in nothing. The run comes home at the first frame start after it ends: the
layer goes back to its place, its report to the DCC, and what it staged lands at
that frame's `Boundary`. GORNA lends it an envelope like any family, but its time
is not on the frame's critical path.

A run may outlive what its work was for: a project closed while its scripts
compiled, an asset unloaded while it decoded. So a staged output can hold to a
**lease** (`Lease`), a condition it depends on — the project still open, the asset
still loaded. When the run comes home, the outputs whose lease no longer holds are
abandoned instead of landing. What the app itself staged since the last frame
start (a mount, an unmount) lands after the runs that came home, so the app's
action has the last word over a run it overtook.

The same pool prepares the layers agents switch to, one per family at a time, while
the installed layer keeps running. A prepared layer is taken in at the next frame
start and swapped in; one that failed to prepare wakes its agent
(`PrepareFailed`).

## The Substrate Pass

The **Substrate** is the data layer presenting itself to the frame: the
invariants that keep the World consistent (stage 2, and `Maintenance` at stage 6)
and the projection Flows that publish read-only Views for the lanes (stage 4). The
scheduler *calls* the Substrate because it owns the order of the frame; it does
not decide how the data is laid out — that is the data layer's own concern.

Flows are **read-only projectors**. A Flow reads the World and publishes a typed
View; it never changes game state and never changes which components an entity
has. This is the frame-level form of *adapt the HOW, never the WHAT*. Lanes read
those Views from the bus rather than querying the World, which is what lets the
data layer change its memory layout without any lane noticing.

## Fixed-timestep simulation and render interpolation

The screen refreshes at whatever rate the machine manages; the **simulation** must
advance in fixed increments to give the same result on a 30 Hz machine and a
144 Hz one. Khora reconciles the two with one time **accumulator**, owned by the
scheduler. Each frame:

1. **Measure and clamp.** The real time since the previous frame, at most
   `MAX_FRAME_DELTA_SECONDS` (0.25 s). A longer gap — a debugger break, a hitch, a
   window drag — is cut, so the accumulator never demands an unbounded catch-up.
2. **Scale, then accumulate.** The clamped time times `Time::scale` (see
   [The simulation clock](#the-simulation-clock)), added to the accumulator.
3. **Take whole ticks.** The game's tick, `DEFAULT_FIXED_DELTA_SECONDS` (1/60 s), is
   the one fixed clock game code sees: as many whole ticks as the accumulator
   holds, at most `MAX_SIM_STEPS` (5, 83 ms of game time). The rest carries over.
4. **Derive alpha.** The rest as a share of a tick, always in [0, 1), is the render
   `interpolation_alpha`.
5. **Sub-step the fixed families.** A family whose place holds a fixed step —
   physics, at the game's `physics.step` — gets exactly the game time the ticks
   advanced, in a sub-clock of its own, and runs once per whole step of its own
   size. Then the phases run once for everyone else: physics integrates *N* times
   while the frame renders once.

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../images/frame/ticks.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>Measure.</strong> A slow frame took 40 ms: under the 250 ms clamp, at a scale of 1. With the 5 ms the last frame carried, the accumulator holds 45 ms.</li>
<li><strong>Whole ticks.</strong> 45 ms holds 2 ticks of 1/60 s. At most 5 a frame; past them the time is dropped.</li>
<li><strong>The rest.</strong> 11.7 ms is carried to the next frame. As a share of a tick it is the render alpha, 0.70.</li>
<li><strong>Physics.</strong> It gets exactly the 33.3 ms the ticks advanced, cut at its own step, here 1/120 s: 4 steps. <code>FixedUpdate</code> runs twice, once per tick.</li>
<li><strong>Render once.</strong> Each body is drawn 70 % of the way from its previous to its current pose.</li>
</ol>
</figure>

**The guarantee:** the same total elapsed time produces the same number of ticks
whatever the frame cadence. A 144 Hz burst and a 30 Hz stutter spanning the same
time run the same ticks. Game logic that must be deterministic never sees a
variable step; variable-rate code reads the real delta; render smoothing reads the
alpha.

When more than 5 ticks would be due, the excess time is dropped rather than queued.
Under sustained overload — below 12 frames a second — the game runs in slow motion
rather than freezing in an ever-deepening catch-up. A brief slowdown is
recoverable; a spiral of death is not. The cap counts ticks, so it is the same
83 ms whatever step physics runs at.

**Several fixed families.** Physics is the only family the engine ships at a fixed
step, but a game may register more. Each one keeps a **sub-clock** of its own, fed
the same time the ticks advanced and cut at its own step, so two of them may be
owed a different number of steps in one frame. They then run in **rounds**, as many
as the most steps any of them is owed: in each round, every fixed family still owed
a step runs once, phase by phase. A fixed family owed no step this frame counts as
done, so a family that runs `after` it still runs.

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../images/frame/rounds.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>One tick.</strong> The frame advanced one tick, 16.7 ms. Each fixed family's sub-clock gets exactly that: here a game's <code>steering</code> family at 1/30 s, and physics at 1/120 s, which runs <code>after</code> it.</li>
<li><strong>Cut at each step.</strong> Physics holds two steps of 8.3 ms. Steering's 16.7 ms is not one step of 33.3 ms: nothing is owed, and the time is carried.</li>
<li><strong>Owed nothing, counted done.</strong> Two rounds, as many as physics is owed. Steering does not run, but counts as done, so physics, after it, runs in both rounds.</li>
<li><strong>The next tick.</strong> Steering's clock now holds 33.3 ms: one step. Physics again holds two.</li>
<li><strong>Round by round.</strong> Round 0 runs steering, then physics; round 1 runs physics alone. Then the phases run once for every other family.</li>
</ol>
</figure>

A family switched off keeps no sub-clock: switched on again, it is owed nothing for
the time it sat out. The step is read from the place each frame, so a step changed
there takes effect the next frame.

**A step is longer than zero.** A run at a step of zero would advance no time, and
no sub-clock is ever owed one. Boot refuses a registration with `fixed_step:
Some(Duration::ZERO)` (`BootError::ZeroFixedStep`, naming the family), and a place
refuses a zero step set at run time, directly or in the terms an agent lends: it
keeps the step it holds and logs a warning. `None` is how a family runs every
frame.

**A short step costs runs, not time.** The 5-tick cap bounds the game time a frame
advances, never a fixed family's steps: its sub-clock gets exactly the ticked time,
however many steps that holds, so the family never falls behind the game's clock.
A tiny step only multiplies the runs — up to 83 ms / step a frame. If those runs
take longer than the frame's real time, the cap drops real time and the game runs
in slow motion, the family still in step with it. The physics step has a floor
for that reason (`PhysicsSettings::MIN_STEP`, 0.5 ms); a game's own family is
given none.

### The simulation clock

The time that reaches the accumulator is the real one **times `Time::scale`**. At
`0.0` the accumulator never fills, so no tick is taken: no body integrates, no
script timer counts down, and rendering carries on at real time because it reads
the alpha, not the ticks. Between 0 and 1 it is slow motion; above it, fast
forward.

That one number is how the editor pauses a game. `PlayMode` stays inside
`khora-editor` and never becomes engine vocabulary: the editor writes a scale,
exactly as a pause menu or a cutscene would. `Time::scale` is private and set only
through `set_scale`, which clamps at zero: running a solver backwards is not a
slower forward, and every integrator here assumes time moves one way.

The scale says whether time passes; it does not say which families run — that is
the [activation](#which-families-run). Pause leaves scripting, physics and audio on
and sets the scale to 0: a paused game is still the game, its scripts run at a
delta of zero. The scale is applied once, in the scheduler; an agent deciding for
itself whether time passes would be a dozen places to disagree about what "paused"
means.

### Precision is not time

The physics step is a game setting — `physics.step` in the game's settings
(`khora_core::physics::PhysicsSettings`, 1/60 s by default) — and never a GORNA
lever: the simulation's result depends on it, so no budget may change it. It is a
choice of **precision**: how finely a second is cut, never how many seconds pass.
The game time is decided once, by the ticks; the physics sub-clock only divides
it. Whatever the step, a body falls the same distance in a second of real time and
stays within one physics step of the game's clock.

The two clocks are apart on purpose. If the physics step *were* the clock, a
script's `FixedUpdate` would run 30, 60 or 120 times a second depending on the
physics precision, and the catch-up cap — counted in steps — would be 166 ms of
time at one step and 42 ms at another. Bevy (`Time<Fixed>` and physics sub-steps),
Unity (`fixedDeltaTime` as a project setting, a catch-up guard in seconds) and
Godot (`physics_ticks_per_second`) draw the same line: the game owns the tick,
precision subdivides it.

All of this timing state lives in one place, the engine's **`Time`** resource
(`khora_core::time::Time`), written fresh each frame before the phases read it. It
carries the real `delta_seconds`, the `fixed_delta_seconds`, the
`interpolation_alpha`, the frame's `fixed_steps`, and a monotonic `frame` counter.

`fixed_steps` is what a script's `FixedUpdate` is called for: the game ticks the
frame took — never the physics sub-steps, so a script's cadence does not follow
the physics step. A game with no physics has the same fixed clock, and a paused one
takes no tick. Scripts run once per frame, after the fixed sub-steps, on one view
of the world: their `FixedUpdate` is a fixed clock, not interleaved with physics,
and what it writes reaches physics the next frame.

### Why interpolation is render-only

The simulation steps at a fixed cadence and the screen refreshes at another, so the
latest simulated pose rarely lands on a frame. Drawing it raw would judder. The
render path instead blends each body's *previous* and *current* poses by
`interpolation_alpha`, so motion is smooth at any refresh rate.

The previous-pose store is an engine **resource**
(`khora_core::interpolation::TransformInterpolation`), **not** an ECS component. A
post-simulation system snapshots each simulated body's world transform into it
before propagation overwrites it; the render path reads it to blend.

This is *adapt the HOW, never the WHAT* in its purest form. Interpolation is a
render representation with no game meaning, so it never appears as a component in
the inspector or in a saved scene. Only simulated entities (those with a rigid
body) are snapshotted; everything else renders at its current transform. The store
is pruned each pass, so a despawned body leaves no stale entry, and a saved scene is
the same whether or not the renderer ever interpolated it.

## Under the hood

For contributors: the frame driver is `khora-sdk/src/engine/` (`frame.rs`,
`app_update.rs`, `render_frame.rs`) and `khora-sdk/src/winit_adapters.rs`; the
scheduler's frame is `khora-control/src/scheduler/frame.rs`, its clocks
`clocks.rs`, `sim_steps.rs` and `sub_steps.rs`, the background `background.rs`.

**The orders.** A phase's families are sorted by their `after` names, then by
importance (`roster.rs`). A phase's data systems are sorted by their `runs_after`
names, ties broken by `(order_hint, name)` (`substrate/data_system_runner.rs`). Both
sorts are topological and keep their base order among what no edge orders. Among
the families a cycle never reaches the sort: boot refuses one
(`BootError::AfterCycle`, `khora-core/src/agent/after_checks.rs`), and the agent
registry refuses any agent whose Hard dependencies would close one
(`agent_registry.rs`), so every phase has an order. Among the data systems a cycle
is not refused at boot: the scheduler logs an error and runs the base order,
`(order_hint, name)`.

**The pools.** Two pools of threads, each built once with the scheduler. The waves'
pool has the cores less two (the main thread's and the DCC's), between 1 and 16
threads. The background pool, apart so that a long run never queues in front of a
wave, has a quarter of the cores, between 1 and 8.

**Leases are checked on the host's thread.** The reserve (`Reserve`,
`khora-core/src/lane/plumbing/`) checks a staged output's `Lease` when it receives
the output: at a frame start, on the thread that runs it, the same thread as the
app's actions. A lease therefore cannot race the action that breaks it: an action
taken before the check is seen by it, one taken after comes behind it. The
reserve's own lock is not held during the
check, so a lease's condition may take a lock of its own, as a leaf.

The arithmetic, as the scheduler runs it:

```rust
let dt = real_dt.min(0.25) * scale;               // clamped, then scaled
let acc = accumulator + dt;
let ticks = (acc / tick).floor().min(5.0);        // whole ticks, capped
// past the cap, the excess is dropped so the accumulator stays below one tick
let alpha = (rest / tick).clamp(0.0, 1.0);        // render blend factor

let sub_acc = sub_accumulator + ticks * tick;     // exactly the ticks' time
let sub_steps = (sub_acc / physics_step + 1e-3).floor(); // a hair of slack, paid back
```

**Notation.** Frame *n* brings a clamped, scaled time *dₙ ≥ 0*. Write *τ* for the
tick (1/60 s), *aₙ* for the accumulator after frame *n* (*a₀ ∈ [0, τ)*), *kₙ* for
its ticks and *K* = 5 for the cap.

**The step.** *kₙ = min(⌊(aₙ₋₁ + dₙ)/τ⌋, K)*. When the cap does not bind,
*aₙ = aₙ₋₁ + dₙ − kₙτ*; when it does, the excess is dropped and *aₙ* is
*aₙ₋₁ + dₙ − Kτ* reduced modulo *τ*.

**Invariant.** *0 ≤ aₙ < τ* for every *n*, so *αₙ = aₙ/τ ∈ [0, 1)*. *Proof.* With
the cap not binding, *kₙ* is the floor of *(aₙ₋₁ + dₙ)/τ*, so the remainder lies in
[0, τ). With it binding, the reduction modulo *τ* lies in [0, τ) by definition.

**Theorem (cadence independence).** If no frame among *1 … N* hits the cap, the
ticks taken depend only on the total time:

> *Σₙ kₙ = ⌊(a₀ + Σₙ dₙ)/τ⌋* and *a_N = a₀ + Σₙ dₙ − τ Σₙ kₙ*.

*Proof.* By induction on *N*, from *aₙ + kₙτ = aₙ₋₁ + dₙ*: summing gives
*a_N + τ Σ kₙ = a₀ + Σ dₙ*; since *0 ≤ a_N < τ* (invariant), *Σ kₙ* is the floor
of the right side divided by *τ*. Two cadences spanning the same total time
therefore take the same number of ticks.

**Bounds.** A frame takes at most *K* = 5 ticks, 83 ms of game time; with
*dₙ ≤ 0.25 s* and *aₙ₋₁ < τ*, the time dropped in one frame is less than
*0.25 + τ − Kτ ≈ 0.183 s*. The
physics sub-clock receives exactly *kₙτ* each frame and keeps its carry in
[−10⁻³·h, h) for a step *h*: the slack absorbs `f32` rounding, so the steps never
cover more time than the ticks advanced, and never fall a step behind. The results
above hold in exact arithmetic; the scheduler computes in `f32`, whose rounding the
sub-clock's slack is there to absorb.

## Next steps

- **[Agents and Lanes](./agents-and-lanes.md)** — layers, places and waves in
  detail.
- **[Debug a frame](../how-to/debug-a-frame.md)** — walk a live frame through the
  stages and inspect a GORNA decision.
- **[The big idea](./saa.md)** — the cold path that lends the envelopes.
- **[Glossary](../reference/glossary.md)** — every Khora term in one place.
