# AGDF: adaptive data layout

<p class="kp-lede">A cook keeps the knives in one drawer and the spices on one shelf, but a baker who only ever needs flour, sugar and butter lines those three up side by side. Neither changes what the ingredients are; each changes where they sit, to fit the work. AGDF is Khora doing the same with the game's data: it can store the same values in a different order, to fit the processor and the loop that reads them, without ever changing what they are.</p>

**AGDF** — *Adaptive Game Data Flows* — adapts the *representation* of the engine's
data to the hardware and the access pattern. This page opens with what anyone can
follow, then goes down into the mechanism for engine contributors. For the
storage model it builds on, see [Data and the ECS](./ecs.md); to opt a component
into a field-split column, [How-to: add a component](../how-to/add-a-component.md).

---

## In one minute

Component storage can be laid out for the access pattern — one contiguous stream
per field for a compute-heavy batch, whole structs back to back for everything
else — instead of one fixed representation chosen once and forever.

One guarantee governs it, and it runs through the whole engine: **adapt the HOW,
never the WHAT.** A layout change rewrites *how* a component's bytes are stored;
it never changes *which* components an entity has or how the simulation behaves.
Detaching a `RigidBody` far from the camera would change the simulation, so it is
the WHAT, and AGDF never does it; such gating is the game developer's decision.

AGDF has three layers:

<div class="kp-figure-frame">

{{#include ../images/data/agdf-layers.svg}}

</div>

- **Layer 1 — observe and advise.** The DCC watches the data layer through
  telemetry and produces a layout recommendation per component. It never touches
  the data.
- **Layer 2 — the field-split substrate.** A component can opt, when it is
  registered, into a column with one stream per field, and feed SIMD kernels.
  This is the lever that ships.
- **Layer 3 — online repack**, switching a populated component's layout while the
  game runs, is not built; the reasons are below.

Beside them sits the first piece of data-*flow* adaptation that runs every frame:
the Flows' view cache.

---

## Deep dive

### The industry gap

Every mainstream archetype ECS (Unity DOTS, Unreal Mass, Bevy, flecs) stores
components SoA across entities but AoS within the component — one array of whole
structs. The level left alone is the *field*, and no shipping engine adapts layout
online. AGDF works there, under the DCC: [GORNA](./gorna.md) adapts the HOW of
*work* (which level a family runs at), AGDF the HOW of *data*.

### Layer 1 — observe and advise

The DCC runs on its own thread and never holds the `World`. What it learns about
the data layer travels as telemetry, sent from the frame with a non-blocking
`try_send` (telemetry never stalls the frame):

- after each agent's run, its **report** (`TelemetryEvent::AgentReport`): the time
  it measured on each device and its **workload** — the host time, in
  microseconds, that its lane's level quoted on that frame's inputs. The DCC reads
  the run's time as the largest of its devices' times;
- every 60 frames, an **access snapshot** per component (`ComponentAccess`):
  `type_name`, `size_bytes`, `query_count`, `rows_scanned`.

Three consumers ride that tunnel:

- **The cost model** (`khora-control/src/cost_model.rs`) fits each agent's
  measured `(workload, time)` pairs to a complexity class — constant, linear,
  *n* log *n*, quadratic — by least squares. Its best fit tells the planner how
  the agent's cost grows; the forecast frame time at the current workloads is
  compared with the frame's budget: past it, the DCC tightens the target before
  the frame overruns (by at most half) and starts a new negotiation (a
  `ForecastBreach` epoch). See [GORNA](./gorna.md).
- **The layout advisor** (`LayoutAdvisor`, `khora-data/src/ecs/layout/`) turns a
  component's access counters into a read-only `LayoutRecommendation`:
  `HotColdSplit` for a component of 96 bytes or more, `SimdFieldSoa` for one read
  in batches of 1 024 rows or more per query on average, `KeepSoa` otherwise. The
  thresholds come from the engine's own benchmark: the field-split lever pays on
  large compute-bound batches, the hot/cold split on fat components, the lean
  built-ins want neither. Control observes and advises; Data owns its layout; the
  DCC never repacks.
- **Memory tracking.** `SaaTrackingAllocator` (`khora-infra/src/memory/`) wraps the
  system allocator and reports resident bytes, lifetime bytes and net allocations.
  With a memory budget set, the DCC derives a `memory_pressure` (RAM in use over
  the budget); at 0.95 or more, its safety ceiling caps the frame governor's
  budget multiplier at 0.5, at once. Resident bytes whose coefficient of variation
  passes 0.15 raise an alert: a likely allocation in a per-frame path.

### Flow view caching — per-domain change epochs

A **Flow** is a read-only projector: every frame, before any agent, it reads the
`World` and publishes a **View** — a typed snapshot of what its lanes need — on
the `LaneBus` (see [How-to: add a flow](../how-to/add-a-flow.md)). Most frames,
nothing the projection reads has changed, so running it again is waste — and a
cached View equals a fresh one, so skipping the work changes only the HOW.

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../images/data/view-cache.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>Nothing cached.</strong> <code>AudioFlow</code> folds the World's instance and its Audio and Spatial epochs into a key, projects, publishes, and keeps the View under that key.</li>
<li><strong>Nothing changed.</strong> No value was written and no row moved: the key is the same, and the runner publishes a clone of the kept View.</li>
<li><strong>A <code>Transform</code> moved.</strong> The Spatial epoch rose, the key differs: the Flow projects again.</li>
<li><strong>The same value, written again.</strong> A system recomputing a value every frame writes it with <code>set_if_neq</code>: nothing is written, no epoch moves, the View stays cached.</li>
</ol>
</figure>

Two pieces make it sound. The `World` keeps a
[per-domain change epoch](./change-detection.md#a-whole-domain-at-once): equal
epochs guarantee "unchanged", a raise means only "possibly changed". And a Flow
that can name all of its inputs returns a `cache_key` folding the World's
`instance_id`, the epochs of the domains it reads and a bit-level hash of any
runtime value it reads that has no epoch (the editor's camera, the target's
aspect). The runner adds the family's scope itself. On a key match it republishes
the kept View; `Flow::cache_key` returns `None` by default, and such a Flow
projects every frame.

Which Flows cache is an audit of their inputs. `AudioFlow`, `RenderFlow` and
`ShadowFlow` cache. The others project every frame: `PhysicsFlow` (the simulation
writes every frame), `UiFlow` (the target's size is not in its View), `CameraFlow`,
`ScriptFlow`, `PrefabCatalogFlow` and `ReservationFlow`.

### Layer 2 — the field-SoA substrate, and why it is the lever

A default column is a `Vec<T>`: a loop over one field strides over all the others
and wastes cache lines and SIMD lanes. A **field-split** column (`FieldSoaColumn`,
described by a `SoaLayout`, in `khora-data/src/ecs/soa.rs`) stores one contiguous
`f32` stream per field, which tiles into `f32x8` with no gather:

```text
AoS column:         [ t0 r0 s0 | t1 r1 s1 | t2 r2 s2 | … ]   ← strides over fields
field-split column: tx[ x0 x1 x2 … ]  ty[ … ]  tz[ … ] …      ← one stream per field
```

The engine ships `wide`-based `f32x8` batch kernels in `khora_core::math::simd`,
each with a scalar twin that matches it bit for bit. The benchmark
(`crates/khora-data/examples/layout_bench.rs`) measured, on the reference machine,
a **4.25×** speedup for an in-place field-split quaternion normalise — and a
*loss* the moment the same math scatters its result back into an AoS `Mat4`. That
is the lesson: **the SIMD win is a property of the whole pipeline.** It holds only
while the data stays field-split *resident*; a transpose in and out per frame
costs more than the arithmetic saves. That is why Layer 2 makes the field-split
storage *persistent* rather than transposing per frame, and why
auto-vectorisation alone stops near 2.9× (the floating-point reduction in
`sqrt`/`normalize` is not associative, so the compiler may not reorder it).

The substrate is opt-in per component, `#[component(layout = "soa")]`, for a
struct whose named fields are all `f32`; the derive refuses any other. It lives
*inside* CRPECS, not beside it: a component says how its column is created,
written, migrated and read through a few `Component` hooks that default to the
AoS path, and the derive overrides them for an opted-in component. Storage stays
a type-erased column either way, so pages, domains, despawn and serialization are
untouched. What changes is the access path: a field-split component cannot lend a
`&T` (its bytes are spread over the field streams). It is read by value, through
the `Soa<T>` query term (read-only), or with `World::clone_component`;
`World::get` returns `None` for it. Asking for it through a `&T` term is a
programming error and panics.

### The decision core, and why Layer 3 is deferred

AGDF's planning core is a MAPE-K loop (monitor, analyse, plan, execute, over a
knowledge base), and its pieces are built and tested in
`khora-data/src/ecs/layout/`:

- `Ucb1`, a textbook UCB1 bandit, and `SlidingWindowUcb1`, which scores each arm
  over its last rewards only, for the non-stationary reward a game produces (the
  best layout in a menu is not the best in a fight);
- `DecayCounter`, an access tally that decays geometrically each tick;
- `net_reward(benefit, migration_cost)`, which charges the repack to the reward,
  and `should_switch(incumbent, challenger, hysteresis)`, which switches only past
  a margin, so a learner cannot thrash between two near-equal layouts.

All are deterministic (no random numbers), so a decision can be replayed.

They are **not** wired to a runtime repack, and that is the line to keep straight.
The query contract is fixed at compile time: an AoS component is read as `&T`, a
field-split one by value. Flipping a *populated* component's layout while the game
runs would break every `&T` that refers to it. A bandit cannot *learn* a layout
online without trying it — which needs the repack that is not there — so the
learning loop would be circular. The only sound runtime repack is within the
field-split family (plain ↔ tiled AoSoA), whose benefit is marginal for pure
streaming. So Layer 2's registration-time choice — the industry's approach — is
what ships, the bandit gets its reward offline in the benchmark, and Layer 3 is
the frontier, with its design anchors recorded: OREO's α-counter for online
reorganisation with a competitive bound, the speculate-guard-deopt pattern of
V8 and HotSpot, and reuse-distance and phase-detection cost models to warm-start
from a playtest.

### The formal part

**The cost model.** For an agent with samples *(nᵢ, tᵢ)* (workload, measured
ms) and a class *f* ∈ {1, *n*, *n* log₂ *n*, *n*²}, the least-squares constant is

> *c_f* = Σ *f(nᵢ) tᵢ* / Σ *f(nᵢ)²*, with residual *R_f* = Σ (*c_f f(nᵢ)* − *tᵢ*)².

The model keeps the class of smallest residual and predicts *t(n) = c_f f(n)*. It
answers nothing until it holds two samples of distinct workloads: one workload
cannot tell the classes apart. When the forecast frame time *T̂* passes the target
*T*, the target becomes *T* · max(0.5, *T* / *T̂*).

**UCB1.** After *N* pulls in all, arm *a* with *nₐ* pulls and mean reward *μₐ*
scores

> *μₐ* + *c* √(ln *N* / *nₐ*), with *c* = √2 by default,

and every arm is pulled once before any is scored. With rewards in [0, 1], UCB1's
expected regret after *N* pulls is *O*(log *N*) (Auer, Cesa-Bianchi and Fischer,
2002). The sliding-window variant replaces *μₐ* and *nₐ* by their values over
the arm's last *w* rewards, trading that bound for tracking a reward that drifts.

**The switching rule.** With hysteresis *h* ≥ 0, a challenger replaces the
incumbent only when its score exceeds the incumbent's by more than *h*. Setting *h*
to the amortised repack cost means a switch is made only when it pays for itself
under the current estimates.

**The view cache.** Let a Flow's key be *k = H(id, E_d₁, …, E_dₘ, ρ)*, for the
World's instance *id*, the epochs of the domains it reads and the hash *ρ* of the
runtime values it reads. If the projection reads nothing outside those domains and
values, then equal keys imply equal inputs (up to a collision of *H*), and by the
epoch contract ([Under the hood: change detection](./change-detection-formal.md))
the kept View equals a fresh projection. A key that omits an input breaks the
implication; one that adds an input only lowers the hit rate.

### Prior art

AGDF's contribution is *unifying* models proven elsewhere into a game ECS governed
by the DCC, not inventing self-optimising memory. Layout and SIMD draw on LLAMA
(layout as an exchangeable compile-time mapping), Cabana/Kokkos (AoSoA in HPC),
and Chilimbi / Pettis–Hansen (hot/cold splitting). Online reorganisation draws on
OREO (online reorganisation as a Metrical Task System), database cracking, and
Just-in-Time Data Structures. The decision and control side draws on UCB1 and
SW-UCB, switching-cost bandits, MAPE-K, and the speculate-guard-deopt pattern.

## Next steps

- [Data and the ECS](./ecs.md) — the storage model and the change epochs AGDF
  builds on.
- [GORNA](./gorna.md) — the sibling that adapts work, and the cost model they
  share.
- [How-to: add a flow](../how-to/add-a-flow.md) — write a Flow and its cache key.
- [How-to: add a component](../how-to/add-a-component.md) — opt a hot component
  into a field-split column.
- [Glossary](../reference/glossary.md) — AGDF, field-SoA, layout advisor, MAPE-K,
  change epoch, online repack.
