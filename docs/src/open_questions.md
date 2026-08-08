# Open questions

What this engine does not yet answer, and where the next iteration should go.

- Document — Khora Open Questions v1.0
- Status — Living
- Date — May 2026

---

## Contents

1. Adaptive core
2. ECS and data
3. Agents and lanes
4. Rendering
5. Physics
6. Audio
7. Assets
8. UI
9. Serialization
10. Telemetry
11. SDK and editor
12. Extension model

---

## 01 — Adaptive core

1. **Adaptation modes.** `Learning`, `Manual` (pinned), `Stable` (no opportunistic upgrade), and `Bounded { min, max }` are **implemented** (`AdaptationMode`, settable per agent via `DccService::set_adaptation_mode`). **Replay is now implemented**: `DecisionTrace` records GORNA's per-tick decisions (`DccService::start/stop_decision_recording`) and `DccService::replay_decisions` re-issues them deterministically (bit-for-bit — QA / lockstep / bug repro), bypassing live fit and per-agent mode. Still open: `Calibration` (deliberately explore strategy arms to seed the cost model, then freeze) and `Hinted` (a game→engine semantic-hint channel, e.g. "cutscene"/"combat").
2. **Constraints API.** "In this volume, physics > graphics" is a stated capability without a concrete API. `PriorityVolume` is in the roadmap.
3. **Cross-agent coordination.** Today agents declare hard dependencies on each other (RenderAgent → ShadowAgent). When the dependency graph grows, do we need a richer scheduling model than per-frame topological sort?
4. **Variable cold-path frequency.** ~20 Hz is a default. On low-power targets we may want 5–10 Hz. The trigger model for changing this at runtime is open.
5. **ML-augmented heuristics.** A future heuristic could be a small ML model trained on telemetry. The deployment story (model storage, update cadence) is undecided.
6. **Predictive cost model.** A `CostModel` fits measured `(n, time)` samples to a complexity class (`c·f(n)`) so the DCC can forecast a budget breach ("at this growth rate, the frame budget breaks at ~N entities") instead of only reacting. The loop is now closed: the scheduler publishes live per-agent samples via `TelemetryEvent::AgentCost`, the forecast tightens the target *before* a breach, and the measured costs calibrate agents' quoted estimates inside `GornaArbitrator::arbitrate` (rescaled so the current-strategy option equals the measurement, factor clamped to `[0.25, 4.0]`). Still open: the workload size `n` is the coarse global entity count — per-domain workload refinement is pending.
7. **Frame-time PID extensions.** The global budget multiplier is now closed-loop: a PID (`khora_core::control::pid`) drives the *measured* frame time onto the heuristic-suggested target (thermal/battery/phase shape the target; a hard ceiling caps Critical states). The controller already does derivative-on-measurement + low-pass filter, back-calculation anti-windup, setpoint weighting, and output clamping. Deferred, identified by the design survey: **gain scheduling** (distinct gains per regime, e.g. Critical), a **deadband / hysteresis** to suppress limit cycles on the discrete strategy ladder, **CPU/GPU-load feedforward**, a **variance-aware** term (widen the deadband as stutter rises), and **auto-tuning** (Åström–Hägglund relay) or ML-adaptive gains. Default gains are conservative and hand-tuned; per-target tuning lives in `DccConfig::frame_pid`.

## 02 — ECS and data

1. **Parallel query execution.** Today queries run on the calling thread. The borrow-checker's compile-time exclusivity makes parallelization safe; the policy and API are not yet decided.
2. **Adaptive layout (AGDF — Adaptive Game Data Flows).** AGDF is the adaptation of data *layout* — laying hot component columns out for the access pattern and the hardware (field-split SoA, AoSoA tiling, hot/cold split) instead of one fixed representation. After a deeper survey of the prior art (academic + shipping engines + cross-domain systems), it resolves into **three layers**:

   - **Runtime lever (the build target now): explicit-SIMD batch kernels over field-SoA data.** The shipped kernel `khora_core::math::simd::normalize_quat_batch` measures **~4.25× over its scalar twin** on this machine; auto-vectorisation alone caps near ~2.9× because the FP reduction/`normalize`/`sqrt` path is non-associative and the compiler may not reorder it. **Measured caveat that shapes the design:** the win is a *whole-pipeline* property — it holds only while the data stays field-SoA *resident*. The same `wide::f32x8` math in `compose_trs_to_mat4` (SoA in, AoS `Mat4` out) is **scatter-bound and ~0.9× — a net loss** — because the per-lane transpose-back dominates the cheap quaternion expansion. So the kernels are adopted only by loops that stay SoA end-to-end; a one-off transpose for a single op never pays. This mirrors the whole industry — Unity DOTS, Unreal Mass, Bevy, flecs are all SoA-*across-entities*, AoS-*within-component*, and Unity explicitly pushes field-SoA (`float4`) batching into the hot loop by hand; the unexploited layer everyone leaves on the table is the *field* level, which is exactly where this operates. (Engine evidence: `crates/khora-data/examples/layout_bench.rs`.)

   - **Advisory layer: a read-only layout *advisor*.** The access instrumentation, the `CostModel` (`c·f(n)`), and the decision learner (a deterministic `Ucb1` bandit, arms = candidate layouts) run as **glass-box introspection** that *recommends* which components to opt in and which tiling — never as a runtime repacker. This keeps the DCC's relationship to Data observation-only. Open refinements, all still deterministic: a **sliding-window UCB** (the reward is non-stationary across a session), a **cost-into-reward** term (`reward = benefit − c·migration_cost`, the anti-thrash form from contextual DBA bandits), and **discounted/evaporating** access counters.

   - **Deferred: persistent SoA storage and true online repack.** The resident-vs-scatter finding above makes the case for *persistent* field-SoA storage of opted-in components concrete: a kernel can only stay resident (and keep the 4×) if the column it reads *and* the column it writes are both field-SoA, rather than transposing in and out each frame. Making CRPECS storage and `WorldQuery::fetch` layout-polymorphic is the hard, hot-path blocker — today a column is a type-erased `Vec<T>` the fetch downcasts to, and components like `Transform` are queried by `&T` pervasively, so a persistent field-SoA column forces a by-value query-`Item` ripple. It is deferred as its own controlled change; a transient SoA batch (`TrsBatchSoa`) covers the kernels in the meantime. No shipping engine does online layout switching, so this is genuinely novel **and** has no playbook — it is gated behind a conservative trigger and only justified once the advisor proves a component needs it. The design anchors are recorded: **OREO**'s α-counter (online reorg as a Metrical Task System, provable `2(1+log|layouts|)` competitive ratio + built-in hysteresis) for the cost/benefit gate, and the **V8/HotSpot speculate→cheap-guard→deopt** pattern for safety (a wrong layout guess costs speed, never correctness). Cross-domain bets to test here: a reuse-distance cost model that estimates a layout's miss-rate *without* repacking, SimPoint-style phase detection as the re-evaluation trigger, and an AutoFDO/BOLT-style shipped layout profile to warm-start the advisor.

   Prior art drawn on: profile-guided hot/cold splitting (Chilimbi PLDI'99; Pettis–Hansen PLDI'90), AoSoA layout abstraction (LLAMA, Cabana), online reorganization with worst-case bounds (OREO, ICDE'24), just-in-time data structures (De Wael & Marr, 2015), and the MAPE-K autonomic loop (Kephart & Chess, 2003) — which the DCC is an instance of. Distance-based *gameplay* gating (detaching physics) is **not** AGDF — it is opt-in, developer-authored policy.
3. **Page-size tuning.** Pages start at 8 entries and grow geometrically. Whether 64 or 256 would be better at scale is unmeasured.
4. **`khora-plugins` API.** The plugin model is real but its public API is still settling alongside editor needs.
5. **Flow view-cache signals.** `RenderFlow`/`ShadowFlow`/`AudioFlow` now republish their previous View when their `Flow::cache_key` — per-domain change epochs (`World::domain_epoch`) plus, for the render-side flows, a bit-level fingerprint of the editor viewport override — is unchanged (see [AGDF §07](./concepts/agdf.md)). `UiFlow` and `PhysicsFlow` stay uncached: surface size and hot-reloadable fonts have no change signal a key could fold in, and physics mutates its domains every simulated frame. Folding asset-version signals (hot reload) and a surface-size signal into cache keys is open.

## 03 — Agents and lanes

1. ~~**`asset_lane` and `ecs_lane` should not be lanes.**~~ **Resolved.** Asset decoders are services under `khora-io::asset::decoders`; ECS compaction is maintenance in `khora-data::ecs::systems`. `khora-lanes` now holds exactly the four families that have strategies to negotiate: render, physics, audio, script.
2. **Plugin agents.** Agents are added at compile time via registration. Hot-loaded plugin agents need a stable ABI we have not yet committed to.
3. **Multi-`LaneKind` agents.** Forbidden by current rule. If a future subsystem genuinely needs to coordinate two lane kinds (compute + render in the same pipeline), the rule may need a carve-out.
4. **Async agent work.** Some lanes (asset streaming) want async I/O. The contract for an agent that yields control mid-frame is open.
5. **Lane-level parallelism.** Today lanes run sequentially within an agent's `execute`. For some agents (asset decoders) parallel lane execution is obvious; the contract is undefined.
6. **Shader hot-reload.** Files-on-disk make this trivial in principle. The wgpu pipeline cache invalidation policy is not yet decided.
7. **Asynchronous lanes.** Asset streaming wants `async fn execute`. The current sync-only contract is a known constraint.

## 04 — Rendering

1. **Forward+ tile size and light limits.** Tunable in `forward_plus.wgsl`. Defaults work; the optimal is hardware-dependent and deserves a heuristic.
2. **HDR pipeline.** Currently SDR. HDR target format support exists in wgpu 28.0; the tone-mapping pass and editor color-correctness pass are not yet implemented.
3. **Compute-driven culling.** A compute pass for view-frustum culling would let us skip the per-frame extraction cost in `LitForwardLane::prepare`. Designed, not built.
4. **Render graph.** Considered, deferred. Today the lane order is small enough that explicit dependency declaration is clearer than a graph. We will revisit when the lane count crosses ~10 per frame.
5. **Shader management is not settled.** Every `.wgsl` is `include_str!`-ed into the binary and composed once at startup, so a shader cannot be edited without a rebuild — while `.erg` scripts, meshes and textures all hot-reload. The composition point is a `const` table in `system.rs`, which means adding a pipeline edits engine source rather than declaring anything. And the two raw-string exceptions (`TEXT_WGSL`, `EGUI_WGSL`) exist because their consumers take source rather than a handle. What a shader *is* to this engine — a compiled-in constant, or an asset with a UUID like every other file under `assets/` — is the question underneath all three.

## 05 — Physics

1. **Per-region simulation rate.** "Use Standard near the player, Simplified everywhere else" is a gameplay-relevance policy — *not* AGDF (which is layout only). It must be opt-in and developer-authored; the engine provides the detach/reattach mechanism but never applies it by default. The opt-in API is not built.
2. **Physics state in serialization.** `SerializationGoal::FastestLoad` does not preserve velocities or contacts. Whether to add a "snapshot with physics" goal is open.
3. **Native solver migration.** Roadmap Phase 6. The trait surface is stable enough; the implementation is a multi-quarter effort. `khora-infra::physics::khora` holds a broad phase, a narrow phase and a solver, none of them wired into a `PhysicsProvider` and almost none of them tested.
4. **Choosing a backend at all.** Two physics backends now sit side by side, and nothing can pick between them: three call sites construct `RapierPhysicsWorld` directly. `GraphicsBackendSelector` is not the precedent it sounds like — it selects a GPU adapter *within* wgpu, not between renderers. A real selection point is unbuilt, and the editor's "switch the backend" feature depends on it.

## 06 — Audio

1. **HRTF (head-related transfer function) for headphones.** Better spatialization for headphone users. Library candidates exist; integration is not designed.
2. **Listener selection.** Today, first-registered wins. Multiple listeners (split-screen, recording) need an explicit selection model.
3. **Convolution reverb.** Real-time convolution is feasible on modern hardware; the API for impulse responses is undecided.

## 07 — Assets

1. **Streaming.** Today assets load entirely into memory. Streaming meshes (Nanite-style) and textures (sparse residency) are roadmap items.
2. **Async decoder execution.** The decoder runs on the calling thread. Large assets should use a thread pool — the contract is undecided.
3. **Pack builder.** A working `.pack` builder tool is needed to move releases off `FileLoader`. Designed; in development.
4. **Asset hot-reload.** The VFS layer can detect changes; the policy for invalidating in-flight handles is undecided.

## 08 — UI

1. **In-game UI.** `UiAgent` is currently editor-only. The path to a play-mode HUD is mostly a matter of changing `allowed_modes`, plus deciding the input model.
2. **Animations on UI.** No tween / spring system today. Probably belongs as a separate lane that mutates UI components over time.
3. **Accessibility.** Screen reader hooks, contrast modes. Not designed yet.

## 09 — Serialization

1. **DeltaSerialization.** Roadmap item. Save games and undo/redo both want incremental snapshots. The trait surface is sketched, not implemented.
2. **Physics snapshot goal.** Should there be a `SerializationGoal::IncludePhysicsState` that captures velocities, sleep state, contacts?
3. **Versioned components.** Today, scene format version is tracked in the header. Component schema versions are not. A scene saved against an older component definition may fail to load.

## 10 — Telemetry

1. **Histogram exporter.** Histograms collect, but the export format (Prometheus, OpenMetrics) is not yet committed.
2. **Per-frame trace records.** Tracy integration would be valuable. The telemetry pipeline is compatible; the hookup is undecided.
3. **Telemetry retention.** The DCC reads the latest value. Long-term retention (for replay-after-incident analysis) needs a storage policy.

## 11 — SDK and editor

1. **`khora-editor` dependencies.** The editor depends directly on `khora-agents` and `khora-io` for performance. Justified but a violation of "SDK is the public API." Worth revisiting.
2. **Workspace size.** Eleven crates is comfortable today. At twenty it might not be. The split rule is "per scannable responsibility," but we don't yet have a deterministic threshold.
3. **Service registration API.** Custom services are registered inside the bootstrap closure passed to `run_winit`. The pattern works but isn't formalized — a stable, discoverable surface (e.g., a builder over the registry) is overdue.
4. **Multi-window editor.** Popping the viewport to a second monitor — does the popped window keep its own Spine?
5. **Plugin UI surface.** Third-party plugins need a place to live in the Inspector. The contract is undefined.
6. **Collaboration.** Real-time multi-user editing. No roadmap, but the architecture does not preclude it.

## 12 — Extension model

1. **Agent registration API.** `EngineConfig::register_agent` is illustrative, not stable. Settling alongside `khora-plugins`.
2. **Plugin DLL ABI.** Hot-loaded plugin agents need a stable ABI we have not yet committed to.
3. **Custom phases.** `ExecutionPhase::custom(id)` exists but the surrounding tooling (editor visibility, telemetry naming) is incomplete.

---

*This list is honest. If a question is here, it has not been answered. If it is answered, it moves to [Decisions](./decisions.md).*
