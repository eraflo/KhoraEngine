# Glossary

The vocabulary index. Khora uses a small set of proprietary terms — acronyms for
its architecture, names for its runtime substrates — that recur across every
chapter. This page defines each one in a sentence or two and links to the page
that covers it in depth, so a reader landing anywhere can look up a term without
reading the book front to back.

Terms are listed alphabetically. Where a term is an acronym, it is expanded on
first mention.

---

### AdaptationMode

The per-agent dial that bounds how freely GORNA may change an agent's strategy.
Four variants are enforced today — `Learning` (default, negotiates freely),
`Manual(strategy)` (pins one strategy), `Stable` (blocks opportunistic
up-switches), and `Bounded { min, max }` (clamps the strategy range) — set via
`DccService::set_adaptation_mode`. A death-spiral safety stop can still force the
cheapest strategy regardless. See [GORNA](../concepts/gorna.md).

### AGDF — Adaptive Game Data Flows

The data-layer counterpart of GORNA: the online adaptation of the in-memory
*layout* of ECS component storage (field-split SoA, SIMD tiling, hot/cold
splitting) to the access pattern and the hardware — never the *meaning* of the
data. AGDF is self-optimized inside `khora-data`; the DCC only observes it. See
[AGDF](../concepts/agdf.md).

### Agent

A tactical manager that owns exactly one `LaneKind`, exposes that kind's lane
strategies to GORNA, applies the budget GORNA returns, and dispatches the chosen
lane each frame. An agent implements only the `Agent` and `Default` traits — no
extra methods. It is *not* a controller (the DCC decides global priorities) and
*not* a worker (lanes do the work). Eight ship today: Render, Shadow, Skybox,
Overlay, Physics, UI, Audio, and Script (the last registered for
`EngineMode::Playing` only). See [Agents and Lanes](../concepts/agents-and-lanes.md). Also
called an **ISA** (Intelligent Subsystem Agent).

### Channel&lt;T&gt;

The engine's one reusable one-way queue, in `khora-core::event`. Two axes and
nothing else: how a reader reads (a **cursor** it owns, so several readers see
the same events independently — or a destructive `drain`), and what happens when
it is full (`DropOldest` or `Reject`, with the drop counted rather than
silent). Deliberately *not* a global event bus: each channel is created and
registered by whoever owns that traffic. Replaced four independently-grown
queues, one of which was fully generic and entirely dead.

### CLAD — Control / Lanes / Agents / Data

The concrete crate structure and dependency layering that implements SAA, and the
name of the per-frame descent `Control → Agent → Lane → Data`. Dependencies flow
downward only; the descent names how a budget becomes work each frame: Control
arbitrates, the Agent selects a Lane, the Lane reads projected Views and writes
results back to Data. CLAD is *the how*; [SAA](#saa--symbiotic-adaptive-architecture)
is *the why*. See [CLAD](../concepts/clad.md).

### ComponentProvenance

Who has the right to **write** a component — as opposed to `SemanticDomain`,
which says who consumes it. Declared with `#[component(provenance = …)]`; four
values: `Authored` (the default — a human or game code; saved in a scene, copied
on duplicate, offered in "+ Add Component"), `ToolAuthored` (written by a tool
action such as dragging in the scene tree; saved and copied, not offered),
`Derived` (recomputed by the engine from authored state; never saved, never
copied), and `Runtime` (per-run state that nobody authors and nothing
recomputes). A `Runtime` component is never in a scene; one also declared
`resumable` — a behavior's state companion `Guard#state`, and a
[system](#system-ergon)'s component and its companion, are the kinds today — is
kept by a [game save](#game-save--saverecord) so play resumes where it stopped. It is what
keeps a simulated pose out of a scene file without a line of filtering, because
the serializer honours the axis directly. See [Physics](../concepts/physics.md)
and [Scenes and game saves](../concepts/saves.md).

### Contention

What an agent declares that it competes with others for: the `OutputDeck` slots
it writes, and the `Runtime` resources it **reads** or **locks**. Distinct from
`AgentAccess`, which is about the `World` alone. Two agents share a concurrent
wave only when their contentions are disjoint. Enforced at the access point —
`EngineContext::resource` and `::locked` refuse what was not declared. See
[Agents and Lanes](../concepts/agents-and-lanes.md).

### CRPECS — Chunked Relational Page ECS

Khora's custom archetype-based Entity Component System. Storage is **chunked**
into bounded **pages**, the entity-to-data relationship is **relational**
(entities are generation-checked identifiers, not pointers), and the page is the
unit of iteration, compaction, and serialization. The model makes structural
change cheap, which is what AGDF's adaptive layout requires. See
[Data and the ECS](../concepts/ecs.md).

### DataSystem

A registered, fixed (non-negotiating) unit of Data-layer work the engine runs in
an ordered slot each tick — invariants like transform propagation and GPU mesh
sync, plus maintenance write-backs. New invariants are added data-driven, by
submitting a `DataSystemRegistration` rather than wiring a system manually. A
DataSystem is *not* an agent: it has no strategies to negotiate. See
[The frame](../concepts/the-frame.md) and [TickPhase](#executionphase--tickphase).

### DCC — Dynamic Context Core

The engine's central observer and arbitrator, running on a dedicated background
("cold path") thread at ~20 Hz. It maintains a situational model from telemetry,
runs the heuristic engine, arbitrates the GORNA budget auction among agents, and
sends budgets to the hot-path Scheduler through the `BudgetChannel`. It commands
nothing: it sets budgets and observes Data; agents and Data decide *how*. Lives in
`khora-control`. See [GORNA](../concepts/gorna.md) and
[The frame](../concepts/the-frame.md).

### Death spiral

The failure mode where the engine misses its frame budget for several consecutive
frames, each overrun making the next worse. GORNA treats it as a first-class
concept: a death-spiral heuristic forces the cheapest strategy until the engine
recovers, then returns to the negotiated strategy. A separate "spiral of death"
guard bounds the fixed-timestep accumulator (see
[Fixed timestep](#fixed-timestep--interpolation-alpha)). See
[GORNA](../concepts/gorna.md).

### Declared component (Ergon)

A component a script declares — a `component`, a behavior's fields, a
behavior's state companion (`Guard#state`), a system's fields — registered in the
world while the engine runs, with no Rust type behind it. Its fields carry their
Ergon types (`Duration` and `Angle` included, stored as floats). The editor's
Inspector draws it as a typed card; a declared component no module declares any
more is **orphaned**: kept with its data, flagged, never deleted. See
[Editor — Scripts](./editor.md#07--scripts).

### EngineMode / PlayMode

**`EngineMode`** decides which agents run in a frame: the scheduler reads the
`SharedEngineMode` resource once per frame and skips any agent registered (through
`DccService::register_agent_for_mode`) for other modes. The engine defines
`Playing`; anything else is `Custom(name)`. The script agent is registered for
`Playing` only. **`PlayMode`** (`Editing` / `Playing` / `Paused`) is the editor's
transport state; the editor turns it into an engine mode every frame —
`Custom("editor")` while editing, `Playing` while playing or paused — and into a
time scale. See [The frame](../concepts/the-frame.md#the-simulation-clock) and
[SDK surface](./sdk.md#engine-modes).

### Ergon

Khora's own gameplay language, in `khora-script`. It exists because a budget
needs stopping to be *ordinary*: when its fuel runs out, Ergon suspends at the
next [safepoint](#safepoint) — never mid-expression — and keeps the machine that
suspended, so the next frame resumes rather than restarts. Runs inside a fuel
budget converted from the time GORNA allocated, and degrades by deferring whole
behaviors rather than thinning every one. A script declares behaviors (one
entity's logic) and [systems](#system-ergon) (one rule over the rows of a
[query](#query-ergon)). Scripts run only in
`EngineMode::Playing`. See [Scripting](../concepts/scripting.md) and the
[Ergon reference](./ergon.md).

### Event (Ergon)

What one entity's behavior tells another, declared once —
`event Damaged(int amount, Entity source);` — and raised by name with its
payload: `target.Raise(Damaged(3, this))`. The declaration is the event's
identity and signature: a raise and every `on Damaged(…)` handler are checked
against it when they compile, and a payload the delivery receives that does not
fit — from the engine, or queued before an edit — is dropped and reported, never
a fault of the receiver. Delivered next frame. The engine raises `Touched` and
`Separated`. See the [Ergon reference](./ergon.md#events).

### ExecutionPhase / TickPhase

Two distinct ordered-slot enums, easy to confuse:

- **`ExecutionPhase`** orders **agent** execution within a frame:
  `Init`, `Observe`, `Transform`, `Mutate`, `Output`, `Finalize`, plus optional
  custom phases. Each agent declares the phases it may run in; the Scheduler runs
  them in order. See [Agents and Lanes](../concepts/agents-and-lanes.md) and
  [The frame](../concepts/the-frame.md).
- **`TickPhase`** orders the Data layer's **DataSystem** invariants around the
  app's update — `PreSimulation`, `PostSimulation`, `PreExtract`, and the
  `Maintenance` pass — independent of agent phases. See
  [The frame](../concepts/the-frame.md).

### Field attributes (Ergon)

`[Range(0, 100)]`, `[Tooltip("…")]` and `[Header("…")]` on a field of a
component, a behavior or a system: how the inspector shows it — the bounds an
edit stays between, what hovering it says, the section title above it. The
editor's alone: a script writes outside a range freely, and a value outside it
is shown in amber, never changed until edited. See the
[Ergon reference](./ergon.md#field-attributes).

### Fixed timestep / interpolation alpha

Khora decouples simulation from rendering. The deterministic fixed-timestep
agents (physics) advance in whole `fixed_delta_seconds` steps (default 1/60 s)
driven by an accumulator in the Scheduler, so simulation is frame-rate
independent; rendering happens once per (variable-rate) frame. To stay smooth
between sim steps, the render path blends each simulated body's previous and
current transform by the **interpolation alpha** — the fraction of a fixed step
carried past the last whole step, in `[0, 1)`. The alpha is render-only and never
affects simulation semantics. The clock lives in `khora_core::time::Time`; the
previous poses live in `khora_core::interpolation::TransformInterpolation` (not in
the ECS — it carries no game meaning). The frame's step count is published as
`Time::fixed_steps`, what a script's `FixedUpdate` is called for. See
[The frame](../concepts/the-frame.md).

### Flow

A read-only per-domain projector that derives a typed **View** of the World
(`RenderWorld`, `ShadowView`, `AudioFlow`'s output, …) and publishes it into the
`LaneBus` during the Substrate Pass. Flows never mutate the World and never bid
for the frame budget; they are the *only* legitimate producer of the Views lanes
consume. New flows are registered data-driven via `register_flow!`. Some flows
opt into per-domain change-epoch caching to skip re-projection when nothing
changed. Lives in `khora-data/src/flow/`. See [AGDF](../concepts/agdf.md) and
[Data and the ECS](../concepts/ecs.md).

### Game save / SaveRecord

What a running game is, written as its differences from the scene it started
from: the components it changed or added on the scene's entities, the entities it
made and destroyed, the components it removed, and the runtime state declared
`resumable`. A `SaveRecord` names its base scene by asset id and keeps each
changed component whole beside the scene's value at save time, so loading is a
three-way merge against the scene *as it is now* — an edit made to the scene
since reaches every field the game left alone. Written by
`GameWorld::save_game`, read by `load_game`. See
[Scenes and game saves](../concepts/saves.md).

### GORNA — Goal-Oriented Resource Negotiation and Allocation

The per-tick protocol by which the DCC and the agents trade frame budgets,
replacing static compile-time allocations. Each tick the DCC runs five phases —
Awareness, Analysis, Negotiation, Arbitration, Application — collecting each
agent's strategy options and costs, then handing back a `ResourceBudget` per
agent that reflects *this* hardware, *this* scene, *this* frame. Only agents
negotiate; the Data layer does not. See [GORNA](../concepts/gorna.md).

### Headroom

The slack between current resource use and the budget or safety ceiling. When the
frame-time PID loop sees headroom (measured frame time under target), it climbs
the global budget multiplier back toward 1.0 so agents can upgrade to richer
strategies; under pressure it lowers the multiplier. Memory headroom similarly
gates whether a layout repack is allowed. See [GORNA](../concepts/gorna.md) and
[AGDF](../concepts/agdf.md).

### Interpolation (Ergon)

`$"hp: {h.current}"` — a `string` whose holes are shown by their type: `2.0`,
`2.5s`, `90deg`, `(1.0, 2.0, 3.0)`, `Entity#12`. Built in one allocation. See
the [Ergon reference](./ergon.md#interpolation).

### ISA — Intelligent Subsystem Agent

The SAA name for an [Agent](#agent): a semi-autonomous subsystem manager with
self-assessment, multiple strategies, and cost estimation, negotiating through
GORNA. "Agent" is the trait and the day-to-day term; "ISA" is the conceptual name
used in the architecture and roadmap. See [SAA](../concepts/saa.md) and
[Agents and Lanes](../concepts/agents-and-lanes.md).

### Lane

A hot-path execution unit: one deterministic strategy an agent can choose from
(render a forward pass, simulate one physics step, mix one audio frame). Lanes do
not decide *whether* to run and do not negotiate — they run when an agent
dispatches them. Lanes consume the typed Views the Flows publish into the
`LaneBus` and never query the World directly. The `Lane` trait has a
prepare/execute/cleanup triple. See [Agents and Lanes](../concepts/agents-and-lanes.md).

### LaneBus / OutputDeck

The typed input/output substrate of the lane layer. The **`LaneBus`** is where
Flows publish their per-domain Views for lanes to read (the lane *input* side).
The **`OutputDeck`** is the typed sink lanes write cross-domain results into
(audio and physics write-backs), drained by the Maintenance DataSystems at end of
frame. GPU render passes use a separate sink, the `FrameGraph`. Both live in
`khora-core/src/lane/`. See [The frame](../concepts/the-frame.md) and
[Agents and Lanes](../concepts/agents-and-lanes.md).

### LoadReport

What a load adapted, said out loud: one entry per place where the file and
today's code disagreed — a field that took its default, a field dropped, a name
read through `formerly`, a number widened, a retired component skipped, an
engine-written component skipped, a reference to an entity the file does not hold, a game save's change to an entity
the scene no longer has. None of these is an error and none is silent. A load
that cannot be adapted — an unknown component type — fails instead, and leaves
the world as it was. See [Serialization](../concepts/serialization.md).

### Named argument (Ergon)

`Move(3.0, speed: 2.0)` — an argument given by its parameter's name, after the
positional ones, in any order; a parameter left out takes its constant default.
Arguments run in the order written. See the
[Ergon reference](./ergon.md#calls).

### OnLoad / OnResumeFailed

Two Ergon lifecycle hooks. **`void OnLoad()`** runs once when a behavior instance
is restored from a game save — after its fields are restored,
before anything else its turn runs; not on a scene load nor on Stop, which are
fresh starts and get `OnSpawn`. **`void OnResumeFailed(string member)`** runs once
for a suspended body that no [resume tier](#resume-tier) could take back into the
code as it is now, on the turn where the body would have resumed, naming the
abandoned member (`"OnSpawn"`, `"Patrol.OnHit"`). See the
[Ergon reference](./ergon.md).

### PersistentId

The identity a saved scene knows an entity by, kept by the `World` beside each
live entity (not as a component). Unlike an `EntityId`, it does not depend on
where the entity sits in one running world. Two namespaces share its 64 bits:
**authored** ids, random, given to entities the editor creates so concurrent
edits on two branches do not collide; and **created** ids, numbered by the world
for entities the game or code spawns. Entities inside a
[prefab instance](#prefab--prefab-instance--override) derive theirs from the
instance root's id. A **named** id, `PersistentId::named(text)`, is derived from
a text under a namespace of its own, fixed forever, and lives in the authored
namespace: a [system](#system-ergon)'s entity is `named("system:Name")`, the same
in every save. See [Data and the ECS](../concepts/ecs.md) and
[Serialization](../concepts/serialization.md).

### Prefab / prefab instance / override

A **prefab** is a subtree written down as a scene file (`.kprefab`). A **prefab
instance** is a copy of it in a scene that stays *linked*: its root carries a
`PrefabInstance` naming the prefab, and a scene record keeps the instance as that
link plus its differences rather than as whole entities. Loading expands the
prefab as it is now, so a prefab edit reaches every instance. An **override** is
one of those differences — a field value that differs, a component added or
removed. The editor's inspector marks overrides, reverts them, and applies a
field, a component or the whole instance's overrides back into the prefab. See [Prefabs](../concepts/prefabs.md) and
[How-to: work with prefabs](../how-to/work-with-prefabs.md).

### Query (Ergon)

`foreach (Entity e, ref Health h, Regen r, Shield? s) with Player without Dead
{ … }` — a `foreach` with no `in`, whose body runs once per entity holding every
component bound without `?` and every `with`, and none of the `without`. Each
binding is a copy of the row's component (`T?` is `null` where the row lacks
it); only a [`ref` binding](#ref-binding-ergon), in a system, is written back.
Outside a system a query is read-only. Every component it names joins the
program's reads. See [Scripting](../concepts/scripting.md#queries-and-sweeps)
and the [Ergon reference](./ergon.md#queries).

### `ref` binding (Ergon)

A query binding written `ref Health h`, allowed only in a system and never
optional or of a behavior's component. `h` is a copy whose fields the body may
assign; where the row ends — after the body, where a `continue` lands, before a
`break` or a `return` leaves the sweep — the fields that differ from the row as
read are written into the frame's working set at once and into the run's
pending batch, which leaves as one `WriteColumn` per component and set of fields,
in the order the run wrote them. See [Sweep](#sweep-ergon).

### ResourceBudget

The result GORNA hands each agent: the chosen `StrategyId`, a time limit, optional
memory and VRAM limits, and an extra-params map. Narrow by design — adding a
resource dimension is a deliberate change, not a free-form bag. See
[GORNA](../concepts/gorna.md).

### Resume tier

How a suspended Ergon body comes back after the code it was running in changed —
a hot reload, or a load from a save taken under an older script. Tried in order:
**exact** (the same program), **unchanged** (every function on its stack kept
its fingerprint), **rebuilt** (each frame laid out again at the same named
[site](#site-ergon) in the edited code), **restarted** (its member runs again
from its entry with its original arguments). When none holds, the body is
abandoned and the behavior's [`OnResumeFailed`](#onload--onresumefailed) runs.
See the [Ergon reference](./ergon.md).

### SAA — Symbiotic Adaptive Architecture

Khora's organizing philosophy: subsystems live in *symbiosis* (neither commanding
nor commanded) and the engine *adapts* to its environment continuously rather than
through a configuration step. Every major subsystem is a negotiating agent; a
central observer (the DCC) watches and arbitrates each tick. SAA is *the why*;
[CLAD](#clad--control--lanes--agents--data) is *the how*. See
[SAA](../concepts/saa.md) and [CLAD](../concepts/clad.md).

### Safepoint

A place an Ergon run may stop when its fuel is spent: a statement's start or a
loop's head (marked by a `Safepoint` instruction), a function's entry, or just
after a call returns. Fuel is checked only there, so a machine stopped for fuel
always stands at a place the compiler named, with no half-evaluated expression in
its registers. Between two safepoints a run may spend past its budget — charged
as an overdraft — but never around a loop, because every back edge lands on one.
See [Scripting](../concepts/scripting.md).

### Scene record / SceneRecord

A scene written down page by page, as CRPECS stores it: each page record is a
signature (component names), the [`PersistentId`](#persistentid)s of its rows,
and a column of values per component — values by name, never the memory a
column occupies. A record also lists the prefab instances it keeps as links.
A record is applied back atomically: everything is read and staged before the
world changes, so a file that fails to load leaves the world untouched. See
[Serialization](../concepts/serialization.md).

### Scheduling attributes (Ergon)

`[Critical]`, `[Budget(0.2ms)]` and `[Rate(10hz)]` on a behavior or a system: how
the script lane shares a frame's fuel when it will not cover every one —
critical ones claim first, a budget caps, a rate thins out turns. They change
whether a turn runs this frame, never what it does; and no time is lost to a
frame an instance sits out, its next turn is handed it. See the
[Ergon reference](./ergon.md#scheduling).

### ScriptProblems / Problems tab

What is wrong with the running project's scripts, in one place: compile
diagnostics located at `module:line:col`, registration refusals and orphans,
faulted instances and bodies the [watchdog](#watchdog-ergon) warns about. Each
source — a module's compile, the registration, the running instances —
replaces its own list each time it reports, so a clean compile clears its
module. A `Runtime` resource; the editor shows it as its **Problems** tab and
as badges on `.erg` tiles. See [Editor — Scripts](./editor.md#07--scripts).

### SemanticDomain

The partition a component belongs to, declared with `#[component(domain = …)]` and
carried in the component registry: `Spatial`, `Render`, `Physics`, `Audio`, `Ui`.
Domains let query planners and Flows pre-filter pages (a render extraction never
touches UI pages) and anchor the per-domain change epochs that gate Flow view
caching. See [Data and the ECS](../concepts/ecs.md).

### SerializationGoal / encoding / snapshot

A **`SerializationGoal`** states why a file is being written — fastest load,
smallest file, human-readable, long-term stability, editor interchange, portable
binary — and the `SerializationService` picks the **encoding** that serves it:
text (JSON), compact, or MessagePack, all holding the same scene record by name.
`FastestLoad` is served instead by a **snapshot**: values by position, every
component listed with its schema fingerprint, refused whole by a build whose
schemas differ — a cache of a stable save, never its only copy. The goal changes
how a scene is encoded, never what it holds. See
[Serialization](../concepts/serialization.md) and [File formats](./formats.md).

### Site (Ergon)

A named place in a compiled Ergon function where a frame can stand: its entry, a
statement's start, a loop's head, just after an `await`, or just after a call
returns. The name comes from the source's structure — what a statement says, not
where it sits — so an edit elsewhere in the function leaves it unchanged, and a
frame frozen at a site can be found again in edited code. Each site records the
locals in scope there, by name and type. See [Resume tier](#resume-tier).

### Strategy / StrategyId

A *strategy* is one algorithm an agent can run for its `LaneKind`, embodied by a
lane (e.g. `RenderAgent`'s Unlit / LitForward / Forward+). The agent exposes each
as a `StrategyOption` with a cost estimate during negotiation; the chosen one
returns in the `ResourceBudget` as a `StrategyId`. See [GORNA](../concepts/gorna.md)
and [Agents and Lanes](../concepts/agents-and-lanes.md).

### Substrate Pass

The pre-agent phase of the Scheduler's frame in which every registered Flow runs
and publishes its typed View into the `LaneBus`. It ensures lanes (which run
afterward) read pre-projected, AGDF-adapted Views rather than touching the World.
The Scheduler invokes it because it owns tick ordering — this is orchestration of
*when* Data takes its turn, not control over *how* Data lays itself out. See
[The frame](../concepts/the-frame.md).

### Sweep (Ergon)

One run of a [query](#query-ergon)'s body over its rows. The rows are taken once,
when the sweep starts — in entity order, from the frame's working set — and held
by the machine, so a structural change in the body is a command that leaves the
sweep alone; each row is checked again when reached and skipped if it no longer
matches. A sweep short of fuel stops at a safepoint like any body, even
part-way through a row, and resumes next frame on that row; the rows it ended
leave with the run, each written once. See
[Scripting](../concepts/scripting.md#queries-and-sweeps).

### System (Ergon)

`system Name { … }` — a behavior with no subject: one rule over the entities its
[queries](#query-ergon) match. No `this`, no `Position()`, no states, no `on`
handlers, no `async`/`await`, no `OnSpawn`/`OnDespawn`; fields, `Update`,
`every`/`after`, methods, `OnLoad` and `OnResumeFailed`. It runs as a behavior
instance on **its one entity** — spawned by the `script_systems` data system
while the game plays, known by the [named id](#persistentid) `system:Name`,
holding the system's component and its state companion, both `Runtime` and
resumable: never in a scene or a snapshot, kept by a game save. Behaviors and
systems take their turns in one list, by module path, then source order. See
[Scripting](../concepts/scripting.md#a-system-at-a-glance).

### Vessel

The SDK's entity-spawn builder. It guarantees every spawned entity has a
`Transform` and a `GlobalTransform` and lets you attach components fluently
(`Vessel::at(world, pos).with_component(c).build()`). The primitive helpers
(`spawn_plane`, `spawn_cube_at`, `spawn_sphere`) return a pre-loaded `Vessel`. See
[SDK surface](./sdk.md) and [Your first game](../tutorials/your-first-game.md).

### View

The read-only, typed projection of one domain's World state that a Flow produces
and publishes into the `LaneBus` for lanes to consume (`RenderWorld`,
`ShadowView`, …). A cached View is bit-identical to a freshly projected one — only
*whether* the projection runs changes, never *what* it contains. See
[AGDF](../concepts/agdf.md).

### Watchdog (Ergon)

What bounds a body that never finishes — a loop with no `await`, cut by fuel
frame after frame. Per instance it counts the game time the body under way has
spent running without finishing — a frame its behavior sits out repaying fuel
that body overdrew included (a frame it waits on an `await`, or is not run for
any other reason, adds nothing; resuming from an `await` restarts the count). At
`WATCHDOG_WARN` (10 s) it is reported once, naming the member and its site; at
`WATCHDOG_FAULT` (60 s) the instance faults like any fault — disabled until an
edit to its module. The count is saved with the body. See
[Scripting](../concepts/scripting.md#a-body-that-never-finishes).

---

*See also: [Concepts](../concepts/index.md) for where each idea fits, and the
[Crate map](./crates.md) for where each concept lives in the codebase.*
