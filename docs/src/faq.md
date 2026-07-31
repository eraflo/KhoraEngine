# FAQ

Frequently asked questions — for newcomers deciding whether to use Khora, and for
evaluators trying to place it. Answers are short and link to the chapter that
covers the topic properly; terms are defined in the [Glossary](./reference/glossary.md).

---

### Why SAA — why not just a normal ECS engine?

Because most engines decide their resource allocation at compile time: physics
gets *N* ms, rendering gets *M* ms, budgets baked in. Those numbers are wrong on
every machine that is not the developer's. Khora's **Symbiotic Adaptive
Architecture** replaces the static budget with a per-tick negotiation: subsystems
are agents that declare what they can do at various cost points, and a central
observer (the DCC) hands out budgets that reflect *this* hardware, *this* scene,
*this* frame. It still *is* an ECS engine — CRPECS is the data layer — but the
ECS exists in part to make that adaptation cheap. See
[Principles](./concepts/saa.md) and [Decisions](./decisions.md).

### Is Khora production-ready?

No. Khora is **experimental**. The architecture is stable enough to support a
runnable sandbox, an editor with a play mode, and a large workspace test suite,
but the SDK surface is intentionally narrow and the engine is mid-roadmap. Treat
it as a research-grade engine to study or build prototypes on, not to ship a
commercial title on today. The phased plan — scene/assets, the adaptive core,
tooling/scripting, advanced intelligence, then a native physics solver — is laid
out honestly in the [Roadmap](./roadmap.md), and uncertainties are tracked in
[Open questions](./open_questions.md).

### How do I get started?

Clone the repo, run `cargo test --workspace` to confirm your environment, then
`cargo run -p sandbox` for the shipping demo. The smallest working game is under a
hundred lines and is walked through end to end — app struct, bootstrap closure,
spawning a scene with `Vessel` — in the [SDK quickstart](./tutorials/your-first-game.md).
The full example lives at `examples/sandbox/src/main.rs`.

### Can I swap the physics, audio, or render backend?

Yes, by design — that is the load-bearing reason backend code is segregated. Every
backend in `khora-infra` implements a trait that lives in `khora-core`:
`PhysicsProvider` (physics), `AudioDevice` (audio), and the rendering traits
(`RenderSystem` / `GraphicsDevice`), plus `LayoutSystem` for UI layout. Swapping a
backend means writing a new implementation of the trait, typically as a new
sibling folder under `khora-infra/src/<area>/<backend>/`; the rest of the engine
never sees the change. Today there is **one** shipping implementation per
trait — wgpu, Rapier3D, CPAL, Taffy — so swapping is a supported extension point,
not a menu of ready alternatives. See [Architecture](./concepts/clad.md) and
[Extending Khora](./tutorials/extending-the-engine.md).

### How do I debug why GORNA chose a particular strategy?

GORNA is observable by design — "the engine has a mind; show it." At runtime you
watch decisions in the editor's **Control Plane** mode, whose **GORNA Stream**
panel is a live feed of negotiations (timestamp, subsystem, suggestion,
accept/reject — e.g. "RenderAgent: LitForward → Forward+, reason: GPU pressure").
The underlying signals come from the `TelemetryService`. See
[GORNA](./concepts/gorna.md), [Telemetry](./concepts/telemetry.md), and [Editor](./reference/editor.md).
Note: a richer offline decision *recorder* (the `Replay` adaptation mode and a
DCC/GORNA decision tracer) is roadmap work, not shipped.

### What is the difference between an Agent and a Lane?

An **agent** is a strategist: it owns one `LaneKind`, negotiates a budget through
GORNA, and *selects* which strategy to run. A **lane** is the worker: one
deterministic algorithm (render a forward pass, step physics once) that runs when
its agent dispatches it. The agent owns *selection*; the lane owns *execution*.
`RenderAgent` chooses between the `SimpleUnlit`, `LitForward`, and `Forward+`
lanes. See [Agents and lanes](./concepts/agents-and-lanes.md).

### What is the difference between GORNA and AGDF?

They are twin adaptation loops on different layers. **GORNA** adapts *strategy*:
which lane an agent runs, negotiated per tick against the frame budget. **AGDF**
adapts *data layout*: how an ECS component's storage is arranged in memory
(field-split SoA, SIMD tiling), self-optimized inside the Data layer and only
observed by the DCC. GORNA is competitive (agents bid for the budget); AGDF is
not (Data never bids). Both follow the same observe → decide → apply loop, and
both change only the *how*, never the *what*. See [GORNA](./concepts/gorna.md) and
[AGDF](./concepts/agdf.md).

### Does adaptation change my game's behavior?

No — that is the engine's central guarantee: **adapt the HOW, never the WHAT.**
Automatic adaptation may change *representation* — render strategy, simulation
quality, memory layout — but it must never change *game semantics*: which
components an entity has, or any simulation-observable behavior. Dropping an
entity's physics because it is far away *changes the game*; that is a developer
decision, opt-in and authored by you, never something the engine does on its own.
See [Principles](./concepts/saa.md) (pillar 7) and [AGDF](./concepts/agdf.md).

### What platforms are supported?

Desktop: **Windows, Linux, and macOS** — all three are built and tested in CI
every change, rendering through wgpu (Vulkan / Metal / DX12). WebAssembly and
mobile are **not** current targets — they are not in CI and the engine does not
claim them; mobile and VR appear in the docs only as the *kind* of hardware
diversity SAA is designed to handle eventually, and XR is an explicit later-phase
[Roadmap](./roadmap.md) item. Treat Khora as a desktop engine today.

### What is the minimum supported Rust version (MSRV)?

**Rust 1.91.** It is the lowest stable toolchain that compiles the whole
workspace, enforced in CI. The binding constraint is the `#[derive(Component)]`
macro output in `khora-data`, which uses `const fn TypeId::of` in a `const`
context — that became usable in `const` in Rust 1.91. The project is **stable**
Rust (no nightly features); the workspace crates are edition 2021. The manifest's
`rust-version = "1.91"` is the source of truth.

### How do I add a component, a lane, or an agent?

All three are pure-Rust extension points, and the engine wiring is data-driven (no
manual registration in engine code). Add a **component** with
`#[derive(Component)]` plus `#[component(domain = …)]`, which self-registers it.
Add a **lane** (a new strategy) or an **agent** (a new negotiating subsystem) by
implementing the `Lane` / `Agent` traits. The end-to-end worked example — defining
the lanes, writing the agent, registering it through `AgentProvider` — is in
[Extending Khora](./tutorials/extending-the-engine.md); the contracts themselves are in
[Agents and lanes](./concepts/agents-and-lanes.md).

### How is the engine licensed?

**Apache-2.0.** The whole workspace ships under it; the license is declared once
in the workspace manifest and inherited by every crate, and the full text is in
`LICENSE` at the repo root.

### Where does physics run — every frame?

No. Physics runs on a **fixed timestep** (default 1/60 s), decoupled from the
render rate. The Scheduler keeps an accumulator: each frame it advances the
simulation in whole fixed steps (zero or more, bounded to avoid a spiral of death
under overload), while rendering happens once per frame. To stay smooth between
sim steps, the render path blends the previous and current transforms by the
**interpolation alpha**. So on a fast machine a single rendered frame may run no
physics step (just interpolate); on a slow one it may run several. The clock is
`khora_core::time::Time`. See [The frame](./concepts/the-frame.md) and the
[Glossary](./reference/glossary.md).

### Why is the DCC on a separate thread, and what if its budget is late?

The DCC runs the cold path (~20 Hz) on a background thread precisely so analysis
and negotiation never block the frame loop. The two paths touch only through the
`BudgetChannel`, with **last-wins** semantics: if a budget is late, the previous
one simply stays in effect, and if several arrive between frames only the latest
is used. The hot path never waits on the cold path. See
[The frame](./concepts/the-frame.md) and [GORNA](./concepts/gorna.md).

### Can I override the engine and pin a strategy myself?

Yes, within bounds, via an agent's **`AdaptationMode`** (set through
`DccService::set_adaptation_mode`): `Manual(strategy)` pins one strategy, `Stable`
blocks opportunistic up-switches, `Bounded { min, max }` clamps the range, and
`Learning` (the default) lets GORNA negotiate freely. A death-spiral safety stop
can still force the cheapest strategy in an emergency, regardless of mode. Finer
controls — calibration, deterministic replay, game→engine hints, and spatial
priority volumes — are on the [Roadmap](./roadmap.md). See [GORNA](./concepts/gorna.md).

---

*See also the [Glossary](./reference/glossary.md) for term definitions, and
[Open questions](./open_questions.md) for what the engine has not yet decided.*
