# Architecture (CLAD)

<p class="kp-lede">Picture a newspaper on deadline. The editor decides how many columns each section gets tonight. Each section's head decides how to fill them — a long story, or a short one and a photo. The writers write. And the archive holds the facts everyone works from, filed its own way. Nobody tells the archivist how to shelve, and the editor never writes an article.</p>

Khora is built the same way, in four layers named after their initials:
**Control**, **Lanes**, **Agents**, **Data** — **CLAD**. This page shows what each
layer does, where it lives in the code, and why dependencies only go one way. For
the ideas behind it, read [The big idea](./saa.md) first.

---

## SAA, meet CLAD

Two names describe one engine. [**SAA**](../reference/glossary.md), the Symbiotic
Adaptive Architecture, is the *why*: an engine that adapts to the machine it runs
on, through parts that negotiate rather than obey. **CLAD** is the *how*: the
crates, the layers and the direction of every dependency.

| | The why | The how |
|---|---|---|
| Name | **SAA** — Symbiotic Adaptive Architecture | **CLAD** — Control / Lanes / Agents / Data |
| Form | the ideas | the crates and their layering |
| Concern | an engine that adapts | dependencies that flow one way, data that flows one way |

Every SAA idea has one home in the crates:

<div class="kp-figure-frame">

{{#include ../images/clad/pillars.svg}}

</div>

## The descent: Control → Agent → Lane → Data

The CLAD name spells out the path a frame's command takes:

```
Control ──► Agent ──► Lane ──► Data
       envelope  chooses a row   reads the bus, writes the deck
```

- **Control** (the [DCC](../reference/glossary.md) and [GORNA](./gorna.md)) is the
  editor of the newspaper. It watches what the frames measured, shares the frame's
  time among the families, and lends each one an **envelope**: how much time on
  each device, how much memory. It never picks how a family does its work. It
  also owns the frame's order: the scheduler that runs the phases and calls the
  data layer's systems and Flows.
- **Agents** are the section heads. Each runs exactly one **family**, and every
  family's agent is the same `LaneAgent`, built from the family's
  `register_agent!` line. Within its envelope, an agent picks a **row** — a lane
  at one level — and installs it as a [layer](./agents-and-lanes.md#lanes-are-layers)
  the scheduler runs every frame. It is woken only for an event, and keeps no
  per-frame state.
- **Lanes** are the writers: the actual algorithms, fast and deterministic. A lane
  reads typed Views from the `LaneBus` and writes its family's outputs into the
  `OutputDeck`. It decides nothing.
- **Data** is the archive: the CRPECS storage, the Flows that project it into
  Views, and the memory layout the data layer adapts on its own. Lanes never
  change it structurally; the Flows only read it.

Only agents compete for the frame's budget. The data layer never bids.

## The mapping

| SAA idea | Crate | Role |
|---|---|---|
| **Dynamic Context Core** and **GORNA** | `khora-control` | Watches the measurements, runs the GORNA rounds and the arbiter, the frame governor, the scheduler (lending, layers, waves, the background pool) and the Substrate dispatcher. GORNA's message types are in `khora-core`. |
| **Intelligent subsystem agents** | `khora-agents`, `khora-lanes` | `khora-agents` holds the one `LaneAgent` and the boot that builds one per registration. `khora-lanes` holds the families, one folder each: renderer, shadows, skybox, grid, wireframe, gizmo, ui, audio, physics, script, loading — 11 in all. |
| **Adaptive game data flows** | `khora-data` | CRPECS storage, change detection, the layout it adapts itself, Flows, DataSystems, residencies, scene persistence. Representation only, never game meaning. |
| **Semantic interfaces and contracts** | `khora-core` | The traits and types everyone shares: the lane contract, `Agent`, the GORNA protocol types, the backend traits, math, the `Runtime`. |
| **I/O services** | `khora-io` | Assets, the virtual file system, signed packs, the trust boundary, serialization, scene loads: services, not agents. |
| **Observability** | `khora-telemetry` | Metrics and monitors that feed the DCC. |
| **Hardware and OS** | `khora-infra` | The backends: wgpu, winit, Rapier, CPAL, Taffy, and the `.wgsl` shaders. |

The asset service, serialization and ECS maintenance have no agent: they have no
alternatives to negotiate between. The slow part of loading — indexing, decoding,
preparing a scene off the frame — is a family, `loading`, because it does have
two: rush or pace itself. The full crate-by-crate map is the
[Crate map reference](../reference/crates.md).

## Dependencies flow downward only

The dependency graph *is* the architecture. The crates sit in six tiers, and a
crate depends only on tiers below its own, or, on the floor, on a crate the table
names; a cycle does not build. An xtask test (`xtask/tests/crate_graph.rs`) refuses
any edge outside a crate's tier, and pins the two edges below.

<div class="kp-figure-frame">

{{#include ../images/clad/tiers.svg}}

</div>

| Tier | Crates | Depends on, today |
|---|---|---|
| floor | `khora-core`, `khora-macros` | nothing |
| on the floor | `khora-data`, `khora-control`, `khora-script`, `khora-telemetry`, `khora-infra`, `khora-tool-ui` | `khora-core`; data and script also `khora-macros`; control also `khora-data` |
| middle | `khora-io`, `khora-lanes` | io: core, data, script, telemetry · lanes: core, macros, data, script |
| strategists | `khora-agents` | core, data, lanes, script |
| façade | `khora-sdk` | agents, control, core, data, infra, io, lanes, telemetry |
| apps | `khora-editor`, `khora-hub`, `khora-runtime` | sdk; editor and hub also `khora-tool-ui` |

Two edges are easy to get backwards:

- **`khora-agents` never depends on `khora-infra`.** An agent reaches a backend
  only through a `khora-core` trait installed in the `Runtime`.
- **`khora-infra` depends on `khora-core` alone.** It is a floor tenant that
  implements the core traits, not a layer on top of the engine.

Three consequences shape the whole engine.

**Traits live in `khora-core`, implementations elsewhere.** Every backend in
`khora-infra` implements a trait from `khora-core`. Another graphics backend,
physics solver, audio device or layout engine is a new implementation of the trait
— a new folder under `khora-infra/src/<area>/<backend>/` — and the rest of the
engine never sees the change.

**`khora-control` depends on `khora-data` but never commands it.** The scheduler
calls the data layer's systems and Flows because it owns the order of the frame.
That is deciding *when* the data layer takes its turn, not *how* it lays itself
out.

**`khora-sdk` is the only door for games.** A game depends on the SDK; the SDK
depends on everything else. The editor, the hub and the runtime player are apps
like any game.

## Two relationships of Control — and neither commands Data

Control relates to the rest of the engine in two ways, and neither is a command
to the data layer:

1. **The descent.** `Control → Agent → Lane → Data`. Agents are the only parties
   that negotiate for the frame's budget; the DCC shares it *among agents*. Once
   an agent holds its envelope, it picks a lane and the work descends.
2. **Observation.** Measurements flow *up*: hardware monitors, the agents' reports,
   the frames' busy time and the data layer's access counts feed the DCC. This is
   read-only.

The data layer adapts *itself*. Its memory layout is decided inside `khora-data`
from the access patterns it measures, and bounded by its own cost/benefit test; the
DCC only observes the result. Symmetrically, the DCC never dictates an agent's row:
it sets an envelope, and the agent decides. That is what keeps the architecture
symbiotic rather than autocratic.

## The trait surface

The contracts that hold the engine together are a small set of Rust traits, free
of backend types:

| Trait | Defined in | Implemented by |
|---|---|---|
| `Lane` | `khora-core` (`lane/contract/lane.rs`) | every lane in `khora-lanes` |
| `LaneFamily` | `khora-core` (`lane/contract/family.rs`) | one per family folder |
| `Agent` | `khora-core` (`agent/mod.rs`) | `LaneAgent` only |
| `Residency` | `khora-core` (`residency/mod.rs`) | the residencies in `khora-data` |
| `RenderSystem`, `GraphicsDevice`, `PipelineSystem` | `khora-core` | the wgpu backend in `khora-infra` |
| `PhysicsProvider` | `khora-core` | the Rapier backend |
| `AudioDevice`, `AudioMixBus` | `khora-core` | the CPAL backend |
| `LayoutSystem` | `khora-core` | the Taffy backend |
| `Component`, `Flow` | `khora-data` | every component (by derive), every Flow |

Every seam is a trait; no production path downcasts a `Box<dyn Any>` to find out
what it holds. For the full listing, see the rustdoc.

## Under the hood

**The graph.** Let *G = (C, E)* be the crates and their normal dependencies, and
*tier : C → {0, …, 5}* the tier of each crate (floor 0, apps 5). The rule the
xtask test enforces is

> *(a, b) ∈ E ⇒ tier(b) < tier(a)*, except the one edge within a tier that the
> table names: `khora-control` on `khora-data`.

**Theorem (no cycle).** *G* is acyclic. *Proof.* Along any path, the tier never
rises, and it strictly falls except on an edge from `khora-control` to
`khora-data`; the only within-tier edge leaves control and enters data, and data
has no edge back into tier 1. A cycle would have to return to its start, so it
would need a rising edge, and there is none.

**Why it matters for the frame.** `khora-control` sits below `khora-lanes` and
`khora-agents`, so the scheduler cannot name an agent or a lane: it reaches agents
only through the `Agent` trait of `khora-core`, and a family's lanes only through
the layer installed at its place. The descent is not a convention; the crate graph
forbids the shortcut.

## Next steps

- **[The frame](./the-frame.md)** — the per-frame mechanics: stages, phases,
  waves, the fixed clock. Read this next.
- **[Agents and Lanes](./agents-and-lanes.md)** — the middle of the descent.
- **[Crate map reference](../reference/crates.md)** — crate by crate.
- **[Glossary](../reference/glossary.md)** — every Khora term in one place.
