# Knowledge — Architecture Decisions

Durable decisions and their rationale. Append when a new structural decision lands.

1. **Single-acquire frame lifecycle** — `begin_frame()` acquires the swapchain texture once; all
   agents encode to that same target; `end_frame()` presents. Prevents Vulkan semaphore errors and
   enables correct `LoadOp::Load` compositing.

2. **Lane is the universal pipeline** — every hot-path execution (render, physics, audio, asset, ECS)
   goes through `Lane::execute()`. Agents orchestrate lanes but never do pipeline work themselves.

3. **Typed data-flow via LaneBus / OutputDeck** — lanes read typed Views from the `LaneBus` and write
   typed outputs into the `OutputDeck`. No global mutable state; `Flow::adapt` was removed (Flows are
   read-only projectors).

4. **CRPECS for ECS** — archetype-based, SoA storage, components `'static + Send + Sync`, queries are
   the primary data access pattern. AGDF re-tiles columns (SoA↔AoSoA) as self-maintenance, never
   changing semantics.

5. **Abstract GPU resource IDs** — all wgpu resources behind typed IDs (`TextureId`, `BufferId`, …).
   Raw handles never leak into public APIs.

6. **Agent vs Service** — only subsystems that negotiate a GORNA budget are Agents (Render, Shadow,
   Overlay, Physics, Ui, Audio). Non-negotiating work uses direct services (`AssetService`,
   `SerializationService`, `EcsMaintenance`).

7. **SDK is a façade** — the Scheduler, `BudgetChannel`, and `EnginePlugin` are internal; game code
   only sees the `khora-sdk` public surface.

8. **Substrate Pass owns tick ordering** — the Scheduler runs DataSystem invariants and Flow
   projection before the agent descent; `khora-control` depends on `khora-data` only to *invoke* the
   substrate, not to drive layout.

9. **4 bind groups, always** — render lanes use exactly 4 bind groups (Frame / Object / Material /
   Lighting); new lighting features add bindings to group 3, never a 5th group (wgpu universal baseline).

10. **Shaders as files** — WGSL lives in `shaders/pipelines/` + `shaders/lib/`, composed via
    `ShaderRegistry` + `naga_oil` `#import`; no inline source, no runtime filesystem reads.

11. **Documentation in place, no ADR files** — record decisions in existing docs, not a separate ADR
    section. (Project convention.)
