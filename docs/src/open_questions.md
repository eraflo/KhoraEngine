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

1. **Adaptation modes.** `Learning` (fully dynamic), `Stable` (predictable), `Manual` (locked strategies) are designed but not implemented. The contract for switching between them at runtime is open.
2. **Constraints API.** "In this volume, physics > graphics" is a stated capability without a concrete API. `PriorityVolume` is in the roadmap.
3. **Cross-agent coordination.** Today agents declare hard dependencies on each other (RenderAgent → ShadowAgent). When the dependency graph grows, do we need a richer scheduling model than per-frame topological sort?
4. **Variable cold-path frequency.** ~20 Hz is a default. On low-power targets we may want 5–10 Hz. The trigger model for changing this at runtime is open.
5. **ML-augmented heuristics.** A future heuristic could be a small ML model trained on telemetry. The deployment story (model storage, update cadence) is undecided.

## 02 — ECS and data

1. **Parallel query execution.** Today queries run on the calling thread. The borrow-checker's compile-time exclusivity makes parallelization safe; the policy and API are not yet decided.
2. **Live AGDF triggers.** The architecture supports adding/removing components based on context, but the *policy* — who decides, when, with what hysteresis — is open.
3. **Page-size tuning.** Pages start at 8 entries and grow geometrically. Whether 64 or 256 would be better at scale is unmeasured.
4. **`khora-plugins` API.** The plugin model is real but its public API is still settling alongside editor needs.

## 03 — Agents and lanes

1. **`asset_lane` and `ecs_lane` should not be lanes.** A `Lane` is a strategy variant a GORNA-negotiating agent picks per frame. Asset decoders and ECS compaction have no strategies — they are on-demand or fixed maintenance work. The current implementations as lanes are residual and should be lifted into services (`AssetService` / `DecoderRegistry`, `EcsMaintenance`). See [Roadmap](./roadmap.md) Phase 2 — Architecture refactoring.
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

## 05 — Physics

1. **Per-region simulation rate.** "Use Standard near the player, Simplified everywhere else" is a stated goal of AGDF — the API for it is not built.
2. **Physics state in serialization.** `SerializationGoal::FastestLoad` does not preserve velocities or contacts. Whether to add a "snapshot with physics" goal is open.
3. **Native solver migration.** Roadmap Phase 6. The trait surface is stable enough; the implementation is a multi-quarter effort.

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
