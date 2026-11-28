# Glossary

<p class="kp-lede">A word list for the whole book. Khora names its parts with words of its own, and many of them are everyday words used in a precise sense: a family, a row, an envelope, a place. Look one up here, read its sentence or two, and follow the link to the page that explains it.</p>

Terms are listed alphabetically. An acronym is expanded where it is defined.
The terms of the engine's contract, how a frame's time is shared and spent, are
these: [family](#family), [lane](#lane), [level](#level), [row](#row--menu--floor),
[agent](#agent), [envelope](#envelope), [epoch](#epoch-gorna),
[allocation](#allocation), [arbiter](#arbiter), [shield](#shield),
[layer and place](#layer-and-place), [wake](#wake), [bus and deck](#lanebus--outputdeck),
[residency](#residency), and on the disk's side, the [trust layer](#trust--trust-layer).

---

### Activation

Which families run. The engine has no mode: every registered
[family](#family), scripting and rendering included, starts on, and an application
switches one with `activate(family)` / `deactivate(family)` on the
`Arc<Activation>` resource (`khora_core::agent::Activation`, re-exported as
`khora_sdk::Activation`). A family it does not know is refused (`UnknownFamily`).
Requests land at the frame boundary, when the SDK calls `settle()` once per frame
before the first DataSystem phase, so the whole frame sees one state. A family
off is not asked to offer, is lent nothing, and does not run; the DCC hears of a
change as `TelemetryEvent::ActivationChanged` and opens an epoch.

A family can also be switched on **within a scope**: `scope(family, "Preview")`
runs it on the entities holding the component registered as `Preview` only — the
same work on fewer things, a sound previewed in the editor or one script run
while editing. A scoped family counts as on. The substrate hands the marker's key
to the family's systems and Flows (`SystemContext::scope`, `FlowContext::scope`);
a Flow projects only what its `Selection` admits, and an entity entering the
scope arrives while one leaving it is forgotten, not bid farewell. The scripting
and audio families honour a scope; the physics family does not. See
[Role](#role), [Mailbox](#mailbox) and
[The frame](../concepts/the-frame.md#the-simulation-clock).

### AdaptationMode

The per-agent dial that bounds how freely GORNA may shape an agent's envelope.
Every mode is an envelope shape — `Learning` (default, negotiates freely),
`Manual(row)` (a pinned row the agent is told to run), `Stable` (keeps its envelope
while its row still fits, takes no upgrade), and `Bounded { min, max }` (limits the
rows planned and chosen) — set via `DccService::set_adaptation_mode`. A `Safety`
epoch still lends every agent its floor, whatever its mode (see
[Death spiral](#death-spiral)). See
[GORNA](../concepts/gorna.md#adaptationmode--the-developer-control-surface).

### AGDF — Adaptive Game Data Flows

The data-layer counterpart of GORNA: the online adaptation of the in-memory
*layout* of ECS component storage (field-split SoA, SIMD tiling, hot/cold
splitting) to the access pattern and the hardware — never the *meaning* of the
data. AGDF is self-optimized inside `khora-data`; the DCC only observes it. See
[AGDF](../concepts/agdf.md).

### Agent

A family's strategist: it offers the family's [rows](#row--menu--floor) (every
level of every lane the device supports) to GORNA, is lent an
[envelope](#envelope), chooses the row that fits, and installs it as a
[layer](#layer-and-place); between those it sleeps, and only a [wake](#wake)
calls it. Every family's agent is the same [`LaneAgent`](#laneagent), built from
the family's registration; it implements only the `Agent` trait (`id`, `offer`,
`apply`, `withdraw`, `report`, `place`, `wake`, `execution_timing`,
`contention`), and has no `execute`. It is *not* a controller (the DCC decides
how much) and *not* a worker (lanes do the work). Any agent can be switched off
and on through the [Activation](#activation). See
[Agents and Lanes](../concepts/agents-and-lanes.md). Also called an **ISA**
(Intelligent Subsystem Agent).

### Allocation

The DCC's decision for an [epoch](#epoch-gorna) (`khora_core::agent::gorna::protocol::Allocation`):
each agent's [envelope](#envelope), the agents a policy [pins](#pin--allowed-rows)
to one row or limits to some rows, the trigger that opened the epoch, and whether
a lower frame target could still take something back (`lever`). The DCC publishes
it from its own thread on the `BudgetChannel`; the scheduler takes it at the next
frame start and lends each agent its part (`Allocation::lend_to` calls
`Agent::apply`). An agent the allocation does not name keeps what it holds. See
[The GORNA protocol](../concepts/gorna-protocol.md).

### Arbiter

The part of the DCC that decides an epoch's rows. Each **proposer** plans a row
for every agent: `Greedy` climbs each agent's priced rows by quality gained per
microsecond while they fit, `Exact` searches the rows by branch and bound within
a node budget, starting from the greedy plan. Every plan passes the
[shield](#shield); the arbiter keeps the accepted plan of most weighed quality
(the first proposer's on a tie) and lends it, each envelope its planned cost plus
80 % of the slack. With no plan accepted, the plans admission made, the floors,
are lent. Lives
in `khora-control/src/gorna/arbiter/`. See [The arbiter](../concepts/arbiter.md).

### Asset reference (Ergon)

A value naming an asset by its identity (`AssetUUID`), never its path, typed by
its kind: `Prefab`, `Sound`, `Material`, `Mesh`, `Texture`, `Scene`, `Font`, or a
kind a game registers with an Ergon name. A field
holding one is optional — `Prefab? bullet;` — and is chosen in the Inspector's
asset slot. A script compares it, passes it, and hands it to what uses its kind:
`Spawn(bullet, at: p)`, `e.SetMaterial(m)`. See
[Ergon — values and types](./ergon.md#values-and-types).

### Change detection — ticks, `Mut<T>`, `Changed<T>`

How the data layer knows what changed. The `World` counts **ticks**, one per run of
a DataSystem or a Flow; every write carries its run's tick. Each
page keeps, beside each column, a summary (its latest write), a floor, and — for a
component an **observer** watches — one tick per row. Writes go through the
`Mut<T>` guard, which stamps its row on its first write; readers ask for
`Changed<T>` rows `since` the tick of their last run. No change is ever missed. See
[Knowing what changed](../concepts/change-detection.md).

### Change epoch

One tick per `SemanticDomain`: when one of its values was last written or one of
its rows last moved, read in O(1). Equal epochs guarantee the domain is unchanged;
Flows fold them into their view-cache keys. See
[Knowing what changed](../concepts/change-detection.md#a-whole-domain-at-once).

### Channel&lt;T&gt;

The engine's one reusable one-way queue, in `khora-core::event`. Two axes and
nothing else: how a reader reads (a **cursor** it owns, so several readers see
the same events independently — or a destructive `drain`), and what happens when
it is full (`DropOldest` or `Reject`, with the drop counted rather than
silent). Deliberately *not* a global event bus: each channel is created and
registered by whoever owns that traffic.

### CLAD — Control / Lanes / Agents / Data

The concrete crate structure and dependency layering that implements SAA, and the
name of the per-frame descent `Control → Agent → Lane → Data`. Dependencies flow
downward only; the descent names how a budget becomes work each frame: Control
lends each agent an envelope, the Agent chooses a row (a lane at a level), the
Lane reads projected Views and writes results back to Data. CLAD is *the how*; [SAA](#saa--symbiotic-adaptive-architecture)
is *the why*. See [CLAD](../concepts/clad.md).

### Component shape

What a component looks like to code that cannot read Rust (`ComponentShape`,
published by its registration): its fields by name with their Rust types, the
skipped ones left out — an empty list is a marker — or `Opaque` for one that is
not a list of fields, such as an enum. Scripts read the engine's components
through it. See [Data and the ECS](../concepts/ecs.md#registration).

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
[system](#system-ergon)'s component and its companion are such — is
kept by a [game save](#game-save--saverecord) so play resumes where it stopped. It is what
keeps a simulated pose out of a scene file without a line of filtering, because
the serializer honours the axis directly. See [Physics](../concepts/physics.md)
and [Scenes and game saves](../concepts/saves.md).

### Contention

What an agent competes with others for: the `OutputDeck` slots it writes, and
the `Runtime` resources, services and backends it **reads** or **locks**. No
agent reaches the `World`, so it is the only thing an agent declares, and nobody
writes it by hand: `LaneAgent` derives it from its lanes' typed inputs and
outputs. Two agents share a concurrent wave only when their contentions are
disjoint; reads never conflict with reads. Enforced at the access point —
`EngineContext::resource` and `::locked` refuse what was not declared. See
[Agents and Lanes](../concepts/agents-and-lanes.md#what-an-agent-declares).

### Cost / quote

What running one level costs this frame (`khora_core::lane::cost::Cost`): the
time on each device (CPU, GPU, NPU, other), the bytes sent between host and GPU,
the bytes held on each device, a confidence, and, when it differs from the
level's own, the impact on these inputs and the share of the due work served. A
lane's `cost(level, inputs)` gives it; that answer is a **quote**. The agent
quotes every row again when the running row's quote drifts more than 10 %
(`QUOTE_DRIFT`), when the last full quote is older than 1 s (`QUOTE_MAX_AGE`),
when it stops fitting the envelope, or when the frame's target changes. See
[Families and levels](../concepts/families-and-levels.md).

### Cost book

What the DCC learns of each (agent, row): the correction from the host time the
row was quoted at to the host time it was measured at
(`khora-control/src/gorna/cost_book.rs`). The [arbiter](#arbiter) plans with
corrected costs, so a lane that always under-quotes stops being lent too little.
See [The arbiter](../concepts/arbiter.md#the-cost-book).

### CRPECS — Chunked Relational Page ECS

Khora's custom archetype-based Entity Component System. Storage is **chunked**
into bounded **pages**, the entity-to-data relationship is **relational**
(entities are generation-checked identifiers, not pointers), and the page is the
unit of iteration, compaction, and serialization. The model makes structural
change cheap, which is what AGDF's adaptive layout requires. See
[Data and the ECS](../concepts/ecs.md).

### DataSystem

A registered, fixed (non-negotiating) unit of Data-layer work the engine runs in
an ordered slot each tick — invariants like transform propagation, the
[residencies](#residency), and maintenance write-backs. New invariants are added
data-driven, by submitting a `DataSystemRegistration` rather than wiring a system
manually. Each run is handed a `SystemContext`, whose `since` is the tick of the system's
previous run, so it can read only what changed since (`Changed<T>`). DataSystem is
*not* an agent: it has no strategies to negotiate. Every registration declares
its [role](#role). See [The frame](../concepts/the-frame.md) and
[TickPhase](#executionphase--tickphase).

### DCC — Dynamic Context Core

The engine's budget keeper, running on a thread of its own (the "cold path"),
20 times a second by default (`DccConfig::tick_rate`). It reads the frames'
reports and the hardware, runs its heuristics and the
[frame governor](#frame-governor), opens [epochs](#epoch-gorna), and lets the
[arbiter](#arbiter) decide each one's [allocation](#allocation), which it
publishes on the `BudgetChannel`. It asks the agents for their offers, but never
lends from its own thread: the scheduler lends at a frame start. It commands nothing: it decides how much; agents
and Data decide *how*. Lives in `khora-control`. See [GORNA](../concepts/gorna.md)
and [The frame](../concepts/the-frame.md).

### Death spiral

The failure mode where each overrun makes the next frame worse. The DCC's
heuristics call it one when three or more independent pressures hold at once;
GORNA also does when two or more agents report stalled. Either opens a `Safety`
epoch: every agent is lent its floor, whatever its
[AdaptationMode](#adaptationmode), and the next epochs negotiate back up. A
separate guard bounds the fixed-timestep accumulator: a frame runs at most five
game ticks (`MAX_SIM_STEPS`) and drops the excess time (see
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

### Envelope

What GORNA lends an agent for an [epoch](#epoch-gorna) (`Envelope`): how much
time on each device and how much memory on each device it may use — never which
row to run. It is the cost of the row the [arbiter](#arbiter) planned for the
agent plus its part of the slack, held to that row; the agent chooses within it.
A lane that reads `#[budget]` gets the envelope's time on its busiest device. See
[The GORNA protocol](../concepts/gorna-protocol.md).

### Epoch (GORNA)

One numbered round of the GORNA dialogue: `OfferRequest`, `Offer`, `Allocation`,
then a `Report` after every run until the next one. An epoch opens on a trigger
(a `Trigger`: the first round, pressure, a forecast over the target, the frame
governor's multiplier moving, agents registered or switched on or off, a policy
changed, an agent stalled, overruns, the periodic refresh, the `Safety`
emergency) and is lent to every
agent at the same frame start. Not to be confused with a
[change epoch](#change-epoch). See
[The GORNA protocol](../concepts/gorna-protocol.md#epochs-and-triggers).

### Ergon

Khora's own gameplay language, in `khora-script`. It exists because a budget
needs stopping to be *ordinary*: when its fuel runs out, Ergon suspends at the
next [safepoint](#safepoint) — never mid-expression — and keeps the machine that
suspended, so the next frame resumes rather than restarts. Runs inside a fuel
budget converted from the time GORNA allocated, and degrades by deferring whole
behaviors rather than thinning every one. A script declares behaviors (one
entity's logic) and [systems](#system-ergon) (one rule over the rows of a
[query](#query-ergon)). Scripts run only while the scripting family
(`khora_core::script::SCRIPT`) is on; the editor switches it off while editing. See [Scripting](../concepts/scripting/index.md) and the
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

- **`ExecutionPhase`** orders the families' [places](#layer-and-place) within a
  frame: `INIT`, `OBSERVE`, `TRANSFORM`, `MUTATE`, `OUTPUT`, `FINALIZE`, plus
  the app's custom phases. Each family's registration names its one phase; the
  scheduler runs the phases in order, and the places of one phase in waves. One
  phase is outside the frame: **`ExecutionPhase::BACKGROUND`**.
  A family there has its layer run off the frame, on a pool of its own, when its
  `Take` [mailbox](#mailbox) holds mail — one run at a time, however many frames
  it takes. Its lanes read only `#[take]` (the requests, drained), `#[resource]`
  and `#[budget]`, and hand on only `#[staged]` outputs, kept in the
  [Reserve](#reserve) until a frame's boundary; boot refuses anything else. See
  [Agents and Lanes](../concepts/agents-and-lanes.md) and
  [The frame](../concepts/the-frame.md).
- **`TickPhase`** orders the Data layer's **DataSystem** invariants around the
  app's update — `Boundary` first, then `PreSimulation`, `PostSimulation`,
  `PreExtract`, and the `Maintenance` pass — independent of agent phases.
  `Boundary` runs right after the families are settled and the scheduler's frame
  start (`begin_frame`: the allocation lent, the layers swapped, the background
  runs that ended brought home): its systems apply what the background staged,
  through `SystemContext::staged::<T>()`. See [The frame](../concepts/the-frame.md).

### Family

One job the engine does every frame, with several ways of doing it: drawing the
scene, casting shadows, stepping physics, mixing sound, running scripts. A family
is a type implementing `LaneFamily` (`khora-core/src/lane/contract/family.rs`):
its name, the `AgentId` it negotiates under, its semantics (`Presentation`, a
cheaper level may look or sound worse, or `Exact`, every level gives the same
result) and the outputs every one of its [lanes](#lane) writes. A family is added
with one `register_agent!` line in its `registration.rs`, giving its phase, its
importance (`Critical`, `Important`, `Optional`), the families it runs after, its
fixed step, its default level and its lanes; the engine boots one
[`LaneAgent`](#laneagent) for it. A new subsystem is a family, never a new agent
type. See [Families and levels](../concepts/families-and-levels.md) and
[Add a subsystem (a family)](../how-to/add-a-family.md).

### Field attributes (Ergon)

`[Range(0, 100)]`, `[Tooltip("…")]` and `[Header("…")]` on a field of a
component, a behavior or a system: how the inspector shows it — the bounds an
edit stays between, what hovering it says, the section title above it. The
editor's alone: a script writes outside a range freely, and a value outside it
is shown in amber, never changed until edited. See the
[Ergon reference](./ergon.md#field-attributes).

### Fixed timestep / interpolation alpha

Khora decouples simulation from rendering. The game's clock advances in whole
`fixed_delta_seconds` ticks (1/60 s) driven by an accumulator in the Scheduler,
so simulation is frame-rate independent; rendering happens once per
(variable-rate) frame. The fixed-step families (physics) cut that simulated
time into sub-steps of their own size, held at the family's place
(`Place::fixed_step`; for physics the game's `physics.step` setting, never a
GORNA lever) — a precision, which never changes how much time passes. To stay smooth
between sim steps, the render path blends each simulated body's previous and
current transform by the **interpolation alpha** — the fraction of a fixed step
carried past the last whole step, in `[0, 1)`. The alpha is render-only and never
affects simulation semantics. The clock lives in `khora_core::time::Time`; the
previous poses live in `khora_core::interpolation::TransformInterpolation` (not in
the ECS — it carries no game meaning). The frame's tick count is published as
`Time::fixed_steps`, what a script's `FixedUpdate` is called for. See
[The frame](../concepts/the-frame.md).

### Flow

A read-only per-domain projector that derives a typed **View** of the World
(`RenderWorld`, `ShadowView`, `AudioFlow`'s output, …) and publishes it into the
`LaneBus` during the Substrate Pass. Flows never mutate the World and never bid
for the frame budget; they are the *only* legitimate producer of the Views lanes
consume. New flows are registered data-driven via `register_flow!`. Some flows
opt into per-domain change-epoch caching to skip re-projection when nothing
changed. Every Flow declares its [role](#role) (`const ROLE`). Lives in
`khora-data/src/flow/`. See [AGDF](../concepts/agdf.md) and
[Data and the ECS](../concepts/ecs.md).

### Frame governor

The DCC's loop on the frame's length (`khora-control/src/governor/mod.rs`). It
reads the critical path of every frame, keeps an estimate of the frame, and
moves GORNA's global budget multiplier, which scales the time the DCC shares:
lowered by the target over the estimate when frames run over, raised a quarter
of the way toward its target when they have room, never above its maximum (1.0
by default). Its target is the frame setpoint less its
[headroom](#headroom). It counts frame time, never the wall clock. A moved
multiplier opens an epoch. See
[The arbiter](../concepts/arbiter.md#the-cost-model-and-the-frame-governor).

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

The protocol by which the DCC and the agents share each frame's time, rather
than fixing it when the game is built: per [epoch](#epoch-gorna), each agent
[offers](#offer) its priced rows, the [arbiter](#arbiter) decides, the DCC lends
each one an [envelope](#envelope), and the agent chooses its row within it and
[reports](#report) after every run. The DCC decides how much; the agent
decides how. Only agents negotiate; the Data layer does not. See
[GORNA](../concepts/gorna.md).

### Headroom

The margin the [frame governor](#frame-governor) keeps under the frame
setpoint: its target is `setpoint × (1 − headroom)`, with a headroom of 10 % by
default (`GovernorConfig::headroom`). Frames that fit under the target let it
raise the multiplier, so agents are lent richer rows; frames over it lower the
multiplier. See [The arbiter](../concepts/arbiter.md#the-cost-model-and-the-frame-governor).

### Hot reload

An asset file saved while the engine runs is read again and replaces the old
copy under the same identity, without stopping anything; the last good copy
keeps serving until the new one decodes, and for good if it never does. Scripts
are compiled again instead, and a shader module is replaced by a
[shader overlay](#shader-overlay). See
[Assets — when an asset changes](../concepts/assets.md#when-an-asset-changes).

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

A hot-path execution unit: one deterministic way of doing a [family](#family)'s
work (draw the shadow maps, step physics once, mix one audio frame), offered at
one or more [levels](#level). Lanes do not decide *whether* to run and do not
negotiate. They read typed inputs (`#[derive(LaneInputs)]`: the Views on the
[bus](#lanebus--outputdeck), earlier phases' deck slots, runtime resources,
the budget lent…), write the family's typed outputs (`#[derive(LaneOutputs)]`),
and never query the World. The `Lane` trait (`khora-core/src/lane/contract/lane.rs`)
has `levels`, `supports`, `prepare`, `cost`, `record` and `shutdown`. See
[Agents and Lanes](../concepts/agents-and-lanes.md).

### LaneAgent

The one implementor of `Agent` (`khora-agents/src/lane_agent/`), built by the
boot for every registered [family](#family). It holds the family's menu of
[rows](#row--menu--floor) and their quotes, offers them to GORNA, chooses the row
that fits its [envelope](#envelope), prepares that row's [layer](#layer-and-place)
and installs it at the family's place. The xtask check
`only_lane_agent_implements_agent` keeps it the only one. Without a scheduler, a
test runs a family through `khora_agents::lane_agent::run_frame`. See
[Agents and Lanes](../concepts/agents-and-lanes.md).

### LaneBus / OutputDeck

The **bus** and the **deck**: the typed input and output substrate of the lane
layer, both scoped to one frame. The **`LaneBus`** is where Flows publish their
per-domain Views during the Substrate Pass, with the Views the host hands in
(`HostViews`: the editor's grid, gizmos, wireframe switch), for lanes to read as
`#[bus]` inputs. The **`OutputDeck`** is where lanes write their `#[deck]`
outputs: read by a later phase's lanes as `#[deck]` inputs, or drained by the
Maintenance DataSystems at the frame's end (audio and physics write-backs). GPU
passes go to a separate sink, the frame graph's pass slots (`#[pass]`). Both live
in `khora-core/src/lane/plumbing/`. See
[Agents and Lanes](../concepts/agents-and-lanes.md#how-a-lane-gets-its-data-the-bus-and-the-deck).

### Layer and place

A family's **place** (`Place`) is its fixed spot in the frame — its phase and,
for what draws, its pass. A **layer** is one of its lanes, prepared at one level,
installed at that place: the scheduler runs it every frame without the agent, on
the place's **terms** (the epoch, the envelope, the budget a `#[budget]` lane
reads, the fixed step). A new layer is prepared beside the installed one, off the
frame when it can be, and swapped in at a frame start; the old one is kept ready
three epochs (`KEEP_EPOCHS`), then released. The place turns each run into the
agent's `Report`. See
[Agents and Lanes](../concepts/agents-and-lanes.md#lanes-are-layers).

### Lease

A condition a background run's staged output depends on (`Lease`): the project
still open, the asset still loaded. When the run comes home, an output whose
lease no longer holds is abandoned instead of landing; a scene load whose
lease fails — the world's components changed since it was prepared — is
prepared again. See
[The frame — the background](../concepts/the-frame.md#the-background),
[Serialization — loading while frames run](../concepts/serialization.md#loading-while-frames-run)
and [Reserve](#reserve).

### Level

One set of parameters a lane can run with (`khora_core::lane::level::Level`): its
`StrategyId`, a name for tools and logs ("lit forward", "32 voices"), its
**impact** (how much it changes the result) and the memory it holds on each
device. A lane lists its levels cheapest first. The audio lane offers 8, 32 or
128 voices; the physics lane one level. Each level becomes a
[row](#row--menu--floor) of its family's menu. See
[Families and levels](../concepts/families-and-levels.md).

### LoadReport

What a load adapted, said out loud: one entry per place where the file and
the running code disagreed — a field that took its default, a field dropped, a name
read through `formerly`, a number widened, a retired component skipped, an
engine-written component skipped, a reference to an entity the file does not hold, a game save's change to an entity
the scene no longer has. None of these is an error and none is silent. A load
that cannot be adapted — an unknown component type — fails instead, and leaves
the world as it was. See [Serialization](../concepts/serialization.md).

### Mailbox

What becomes of a family's mail while the family is off. Declared once beside
the reader: `inventory::submit! { Mailbox::of::<T>(family, reader, between) }`
(`khora-core/src/event/mailbox.rs`), where the reader is `Reader::Take` or
`Reader::Cursor(name)` and `between` is `BetweenSessions::Drop` or `Hold`. At each
frame boundary, a family off for the coming frame has its `Drop` mailboxes
emptied, counted by `Channel::discarded()` apart from overflow. Scripting drops
`ScriptEvent` and collisions; the loading family holds its requests
(`CompileRequest`, `SceneLoadRequest`, `IndexRequest`, `DecodeRequest`). A
recompiled module is no mail: it waits in the reserve until scripting is on. See
[Activation](#activation).

### Named argument (Ergon)

`Move(3.0, speed: 2.0)` — an argument given by its parameter's name, after the
positional ones, in any order; a parameter left out takes its constant default.
Arguments run in the order written. See the
[Ergon reference](./ergon.md#calls).

### Observer

A DataSystem or a Flow that watches components for change, declared by the
`observes` list of its registration. The `World` keeps per-row ticks for the
components observers watch and remembers each observer's last-seen tick. See
[Knowing what changed](../concepts/change-detection.md).

### Offer

An agent's answer when the DCC opens an [epoch](#epoch-gorna) (`Offer`): every
[row](#row--menu--floor) it can run, each with its cost, its quality from 0 to 1
and how far the cost can be trusted (`Measured`, `Quoted` or `Assumed`); its
floor; the row running now; and its health. An offer that comes after the
request's `reply_by` is late, and the agent keeps the envelope it had. See
[The GORNA protocol](../concepts/gorna-protocol.md#the-four-messages).

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

### Pin / allowed rows

A developer's hold on an agent's choice, carried by the [allocation](#allocation):
a **pin** names the one row the agent runs (`AdaptationMode::Manual`), **allowed
rows** the only rows it may choose among (`Bounded`). The agent still chooses
within its envelope, among what it is allowed. See
[Families and levels](../concepts/families-and-levels.md#pins-and-allowed-rows).

### PlayMode

The editor's transport state: `Editing`, `Playing`, `Paused`. It is not engine
vocabulary. The editor turns it into two things every frame, in `before_frame`:
the families off (scripting, physics and audio while editing, none otherwise),
through the [Activation](#activation), and a time scale (`0` while paused, `1`
otherwise). See [Editor — Play mode](./editor.md#04--play-mode).

### Prefab / prefab instance / override

A **prefab** is a subtree written down as a scene file (`.kprefab`). A **prefab
instance** is a copy of it in a scene that stays *linked*: its root carries a
`PrefabInstance` naming the prefab, and a scene record keeps the instance as that
link plus its differences rather than as whole entities. Loading expands the
prefab as it is now, so a prefab edit reaches every instance. An **override** is
one of those differences — a field value that differs, a component added or
removed. The editor's inspector marks overrides, reverts them, and applies a
field, a component or the whole instance's overrides back into the prefab. A
script places one while the game runs with `Spawn(prefab, at: p)`. See [Prefabs](../concepts/prefabs.md) and
[How-to: work with prefabs](../how-to/work-with-prefabs.md).

### Query (Ergon)

`foreach (Entity e, ref Health h, Regen r, Shield? s) with Player without Dead
{ … }` — a `foreach` with no `in`, whose body runs once per entity holding every
component bound without `?` and every `with`, and none of the `without`. Each
binding is a copy of the row's component (`T?` is `null` where the row lacks
it); only a [`ref` binding](#ref-binding-ergon), in a system, is written back.
Outside a system a query is read-only. Every component it names joins the
program's reads. See [Scripting](../concepts/scripting/queries.md#queries-and-sweeps)
and the [Ergon reference](./ergon.md#queries).

### `ref` binding (Ergon)

A query binding written `ref Health h`, allowed only in a system and never
optional or of a behavior's component. `h` is a copy whose fields the body may
assign; where the row ends — after the body, where a `continue` lands, before a
`break` or a `return` leaves the sweep — the fields that differ from the row as
read are written into the frame's working set at once and into the run's
pending batch, which leaves as one `WriteColumn` per component and set of fields,
in the order the run wrote them. See [Sweep](#sweep-ergon).

### Report

What a run of a family's layer says back (`khora_core::agent::gorna::Report`),
after every run: the row it ran, the time it measured on each device, the share
of its work it got `done`, its **health** (the lesser of `done` and how well the
run kept to its budget), whether it ran over budget or stalled, and why. The
[place](#layer-and-place) builds it; the scheduler hands it to the DCC, which
learns from it in its [cost book](#cost-book). See
[The GORNA protocol](../concepts/gorna-protocol.md#the-four-messages).

### Reserved entity / EntityReserver

An entity id a script's turn takes before the entity exists: `Spawn` returns it
at once, so the turn can keep it and queue writes on it. The world's
`EntityReserver` hands ids out without a lock; Maintenance commits each one a
spawn accepted and releases the rest, every frame — an index reused gets a new
generation. See [Scripting](../concepts/scripting/writing.md#an-entity-named-before-it-exists)
and [Reserved id / nowhere entity](#reserved-id--nowhere-entity).

### Reserved id / nowhere entity

An entity id taken before the entity exists — by a script's `Spawn`, or by a
scene load prepared off the frame — not alive, seen by no query, and never
handed to anyone else until it is spawned or given back. The **nowhere** entity
is one id each world reserves when it is made and never spends: a saved
reference a load cannot bind points there, so it never comes to name a later
entity. See [Data and the ECS](../concepts/ecs.md#the-storage-model).

### Reserve

What the runs of the families at the background staged and no frame applied
yet, by output type (`khora_core::lane::Reserve`, a runtime resource). A run's
`#[staged]` outputs reach it at the frame start after the run ended — never in
the middle of a frame — and a `Boundary` DataSystem takes them. An output
implements `Staged`: a newer one of the same key (its `Supersedes`) replaces a
waiting one, which goes through `Staged::abandon` and is never applied.
Outputs staged under a `Lease` (`khora_core::lane::Lease`; a `#[staged]` field
of type `Leases<T>`) count only if the lease still holds when the reserve takes
them in. The project a compile was for may have closed while the compile ran:
its result is then abandoned. See
[ExecutionPhase / TickPhase](#executionphase--tickphase).

### Residency

Keeping a copy of an asset where a device can use it: a mesh's vertex buffers on
the GPU, a UI image's rect in the GPU atlas, a material's uniforms. A residency
(the `Residency` trait, `khora-core/src/residency/`) names what it starts from
(its `Source`, a CPU asset), what it makes (its `Resident`) and what it makes it
on (its `Target`). Each runs as a `PreExtract` DataSystem, registered with
`register_residency!` in `khora-data/src/residency/` (`MeshOnGpu`, `ImageOnGpu`,
`gpu_material_sync`); its store remembers which copy of the source each resident
was made from, so a hot-reloaded asset is made again. An asset's copy on a device
is a residency, never a lane's state. See [Residency](../concepts/residency.md).

### Resume tier

How a suspended Ergon body comes back after the code it was running in changed —
a hot reload, or a load from a save taken under an older script. Tried in order:
**exact** (the same program), **unchanged** (every function on its stack kept
its fingerprint), **rebuilt** (each frame laid out again at the same named
[site](#site-ergon) in the edited code), **restarted** (its member runs again
from its entry with its original arguments). When none holds, the body is
abandoned and the behavior's [`OnResumeFailed`](#onload--onresumefailed) runs.
See the [Ergon reference](./ergon.md).

### Role

Whose work a DataSystem or a Flow is, declared by every registration with no
default (`khora_data::ecs::Role`). `Upkeep` is a data invariant and runs every
frame. `Serves(family)` is the family's work, or what that work produces; it is
skipped while the family is off. On its first run on a world, and its first run
after a skip, a system sees `SystemContext.resumed` and a Flow's `select` sees
`FlowContext { resumed: true }`. The boot refuses a `Serves` that names no
registered family. See [Activation](#activation).

### Row / menu / floor

A **row** is one level of one lane of a family, priced: what the agent offers
the DCC and chooses among, named by the level's `StrategyId` (`OfferRow`). The
**menu** is every row the device supports, lane by lane, cheapest level first;
the agent offers it without the rows another row beats on every count. The
**floor** is the cheapest priced row: what the agent runs when nothing fits its
envelope, and what a `Safety` epoch lends. A family never offers one
`StrategyId` twice; boot refuses it. See
[Families and levels](../concepts/families-and-levels.md#the-menu-rows-and-their-prices).

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
See [Scripting](../concepts/scripting/index.md).

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
carried in the component registry: `Spatial`, `Render`, `Audio`, `Physics`, `Ui`,
`Script`.
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

### Shader overlay

A project's own `.wgsl` file under `assets/shaders/` whose path names one of
the engine's shader modules, which it replaces. It is code the graphics card
runs, so it applies only in a trusted project; a restricted one ignores it with
a warning. See [Trust — shader overlays](../concepts/trust.md#shader-overlays).

### Shield

What every proposal for an epoch must pass before the [arbiter](#arbiter) keeps
it (`khora-control/src/gorna/arbiter/shield.rs`): no row a developer's policy
rules out (a pin, the allowed rows, a `Stable` ceiling), and no breach of the
frame's constraints, the critical path on each device against the target and the
memory against what each device holds. A proposal it refuses is set aside, and
the journal names the proposer. The shield is why a better proposer can be added
without risk: whatever it plans, nothing unsafe is lent. See
[The arbiter](../concepts/arbiter.md#the-shield).

### Site (Ergon)

A named place in a compiled Ergon function where a frame can stand: its entry, a
statement's start, a loop's head, just after an `await`, or just after a call
returns. The name comes from the source's structure — what a statement says, not
where it sits — so an edit elsewhere in the function leaves it unchanged, and a
frame frozen at a site can be found again in edited code. Each site records the
locals in scope there, by name and type. See [Resume tier](#resume-tier).

### Strategy / StrategyId

A *strategy* is one way of doing a family's work, embodied by a lane at one level
(e.g. the render family's Unlit / LitForward / Forward+). The agent offers each as a
priced row (`OfferRow`, named by its `StrategyId`) in its `Offer`, chooses the row
that fits the envelope it is lent through `apply`, and names the row it ran in its
`Report`. See [GORNA](../concepts/gorna.md)
and [Agents and Lanes](../concepts/agents-and-lanes.md).

### Sub-asset

One part of a file that holds several, used as an asset of its own — a glTF
file's meshes, materials, textures and scenes. It is named by a label inside the
file (`mesh0/prim1`, `material2`) and gets an identity of its own from the
file's and the label (`AssetUUID::sub_asset`). See
[Assets — glTF files](../concepts/assets.md#gltf-files).

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
[Scripting](../concepts/scripting/queries.md#queries-and-sweeps).

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
[Scripting](../concepts/scripting/behaviors-and-systems.md#a-system-at-a-glance).

### Trust / trust layer

How much the engine trusts a file it reads (`khora_io::trust::Trust`): `Engine`
(its own files), `Project` (a project its user trusts), `Restricted` (one not
trusted yet), `Imported` (a file from outside the project, or carrying the
system's download mark), `Save` (a game save). A project is opened `Trusted` or
`Restricted` (`ProjectTrust`): restricted, its files are read under tighter caps
and nothing of it runs. A refused file is named by its **trust layer**
(`TrustLayer`), where the refusal was decided: `File` (reading it), `Decoder`
(decoding it), `Pack` (reading it out of a signed pack) or `Content` (what it
holds). See [Trust](../concepts/trust.md).

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

### Wake

Why the scheduler calls a family's agent (`khora_core::agent::Wake`). On a frame
without one, the installed [layer](#layer-and-place) runs alone and the agent is
not called. The wakes: the family's first frame, an allocation lent, the running
row's quote no longer fitting the envelope, drifting or aged
([quote](#cost--quote)), the frame's target changed, the lane failed, an input
refused, a layer that could not be prepared. See
[Agents and Lanes](../concepts/agents-and-lanes.md#lanes-are-layers).

### Watchdog (Ergon)

What bounds a body that never finishes — a loop with no `await`, cut by fuel
frame after frame. Per instance it counts the game time the body under way has
spent running without finishing — a frame its behavior sits out repaying fuel
that body overdrew included (a frame it waits on an `await`, or is not run for
any other reason, adds nothing; resuming from an `await` restarts the count). At
`WATCHDOG_WARN` (10 s) it is reported once, naming the member and its site; at
`WATCHDOG_FAULT` (60 s) the instance faults like any fault — disabled until an
edit to its module. The count is saved with the body. See
[Scripting](../concepts/scripting/turns.md#a-body-that-never-finishes).

---

*See also: [Concepts](../concepts/index.md) for where each idea fits, and the
[Crate map](./crates.md) for where each concept lives in the codebase.*
