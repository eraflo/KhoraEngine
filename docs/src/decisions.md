# Decisions

Choices we made, and what we said no to. The global ledger.

- Document — Khora Decisions v1.0
- Status — Living
- Date — May 2026

---

## Contents

1. Architecture
2. Subsystems
3. SDK and editor
4. Process

---

## 01 — Architecture

### We said yes to
- **A self-optimizing core.** GORNA, DCC, and per-tick negotiation are non-negotiable. Without them, Khora is just another engine.
- **Cold path / hot path separation.** The frame loop is never blocked by analysis. Budgets flow one way through a channel.
- **Agent per `LaneKind`.** Render, Shadow, Physics, UI, Audio. One subsystem, one negotiation surface.
- **Trait-defined contracts.** Every seam in the engine is a Rust trait. No string-keyed APIs in the hot path.
- **Splitting `khora-io` from `khora-data`.** Asset loading and serialization are I/O concerns; ECS storage is not.
- **Backends are swappable.** Every `khora-infra` backend implements a `khora-core` trait. wgpu, Rapier3D, CPAL, Taffy are *current defaults*, not architectural commitments.
- **Trait coherence in `khora-core`.** Every public surface seam is a trait. No backend types leak into agents or the SDK.
- **Two threads, one channel.** The DCC owns its thread; the Scheduler owns the main thread; they touch only through `BudgetChannel`.
- **Last-wins budget delivery.** The Scheduler doesn't replay a queue; it reads the latest snapshot.
- **Phase-based ordering.** Agents declare phases, not absolute frame slots. The Scheduler resolves the dependency graph each frame.

### We said no to
- **Static budgets baked at compile time.** A `MAX_LIGHTS` constant has no place in an engine that adapts.
- **Synchronous DCC calls from agents.** Agents must never wait on the DCC.
- **Adding more agents than `LaneKind` variants.** If a subsystem has no strategies to negotiate, it is a service.
- **Mega-crates.** Every crate has a single, scannable responsibility.
- **Sibling dependencies between agents and control.** Agents talk *down* to lanes and *across* to a unidirectional channel — never *up* to control.
- **Dynamic plugin discovery via reflection.** Plugins register through `inventory::submit!` and explicit Rust APIs.
- **A separate "physics tick" loop.** PhysicsAgent owns its accumulator and runs in `Transform` like everything else.

## 02 — Subsystems

### ECS (CRPECS)
- **Yes:** archetype-based storage; bitset-guided iteration; generations on `EntityId`; `#[derive(Component)]` generates the serializable mirror.
- **No:** sparse-set ECS; globally synchronous component change; reflection-driven serialization.

### Agents
- **Yes:** agents implement only `Agent` + `Default` (no extra methods); one agent per `LaneKind`; Hard / Soft / Parallel dependency model; last-wins on `BudgetChannel`.
- **No:** agent-managed concurrency (DCC handles cold-path concurrency); agents reading from each other directly (cross-agent data flows through `FrameContext` slots).

### Lanes
- **Yes:** three-phase lifecycle (prepare / execute / cleanup); type-erased `LaneContext`; `estimate_cost` returning `f32`.
- **No:** lanes referencing each other directly; lane-owned threads; inlined shader source as Rust strings.

### GORNA
- **Yes:** simple, narrow request shape; heuristics as independent functions; per-tick re-negotiation (~50 ms); death spiral as a first-class concept.
- **No:** synchronous negotiation in the hot path; multi-resource vector budgets; GORNA forcing phases.

### Rendering
- **Yes:** strategy-based rendering (Unlit / LitForward / Forward+); shadow as a separate agent; WGSL files on disk; GPU IDs over raw handles; one acquire, one present per frame.
- **No:** a render graph (deferred — current lane order is small enough); inline shader source; backend choice exposed in lane code.

### Physics
- **Yes:** `PhysicsProvider` trait as the single contract; fixed timestep with accumulator; CCD as opt-in per body; strategy includes Disabled.
- **No:** calling Rapier from agents or game code; a separate physics tick loop; variable timestep.

### Audio
- **Yes:** single trait surface (`AudioDevice`); source budget as the primary GORNA dimension; listener tied to ECS; 2D and 3D sources distinguished by flag.
- **No:** calling CPAL directly from anywhere except the backend folder; a "global music" channel; DSP effects in v1.

### Assets and VFS
- **Yes:** UUID-based identity; loose files in dev, pack in release; asset loaders as lanes; reference-counted handles.
- **No:** asset path strings as identity; an "asset agent"; asset hot-reload as a v1 feature.

### UI
- **Yes:** UI components in the same ECS; `LayoutSystem` trait; two-lane split (compute + render); hierarchy via `Parent` / `Children`.
- **No:** an immediate-mode UI inside the engine; a separate UI rendering backend; Taffy types in components.

### Serialization
- **Yes:** three strategies, one file format; `#[derive(Component)]` generates the mirror; play mode uses Archetype; editor uses Definition.
- **No:** reflection-based serialization; a "serialization agent"; preserving physics state across play mode (in v1).

### Telemetry
- **Yes:** telemetry as a first-class service; two collection styles (poll + push); `SaaTrackingAllocator` as the default; string-keyed metric registry.
- **No:** a separate "telemetry agent"; hot-path string lookups for metrics; an external profiler-only dependency.

## 03 — SDK and editor

### SDK
- **Yes:** a small public surface (`EngineCore`, `GameWorld`, `EngineApp` / `AgentProvider` / `PhaseProvider` traits, `run_winit`, `Vessel` + spawn helpers, `WindowConfig`); curated prelude; safe ECS facade; explicit bootstrap closure for renderer registration.
- **No:** hidden global setup; exposing internals (Scheduler internals, GORNA arbitration) through the SDK; a single `prelude::*` that imports everything.

### Editor
- **Yes:** editor as a separate binary; mode-first layouts; play mode through scene snapshot; editor reaches into `khora-agents` and `khora-io` directly (pragmatic shortcut for performance).
- **No:** free-form panel docking; telemetry charts in the main UI (they belong in the Control Plane mode); editor chrome during play mode.

### Extension model
- **Yes:** extension through traits, not callbacks; the agent rule applies to custom agents too; custom strategies as new lanes; backends as trait implementations in `khora-infra`.
- **No:** plugin DLLs at v1; a "lite" Agent trait for simple cases; custom phases as a stable v1 feature.

## 04 — Process

### We said yes to
- **Tests are the contract.** ~470 workspace tests. Adding a feature without a test is a code smell.
- **CHANGELOG is auto-generated.** No human edits.
- **CI runs `cargo xtask all`.** fmt + clippy + test + doc. If it passes there, it passes locally.
- **Documentation ships with the engine.** When the engine changes, the book changes in the same commit.
- **Decisions logged in writing.** This document is the long-form artifact.

### We said no to
- **Pushing without explicit permission.** AI agents and developers alike require explicit user permission to push to `dev`, `main`, or any remote.
- **Skipping git hooks.** `--no-verify` is forbidden unless explicitly requested by the user.
- **Untyped configuration.** No magic strings, no untyped JSON. Configuration is Rust types.
- **A "stable" version of `dev`.** `dev` is the development branch. Stable releases live on `main`.

---

*See [Open questions](./open_questions.md) for the things we have not yet decided.*
