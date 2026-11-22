# Knowledge — Working Memory

Living state of the engine. Update when state changes (build status, latest work, known issues).
Deep history lives in git and `docs/plans/`.

## Current state
- **Branch**: `dev`, clean at `e430f94d`.
- **Build**: `cargo gate` green (~7600 tests, xtask ~160). Treat the live count as truth.
- **In progress**: the agents/lanes contract plan (`docs/plans/agents-lanes-contract.md`), Phase 8 (docs):
  8a and 8b committed, 8c (`.agent/engine` brought to the contract) under way, then 8d–8f.

## Latest work (2026-11-17) — Phase 7 done, conventions enforced, docs site
- **The contract as the code holds it.** A domain writes a family, lanes and a registry line; `LaneAgent`
  is the only `Agent`. Layout: `khora-core/src/lane/{contract,plumbing}/`; `RegisteredAgent` +
  `registered_agents()` in `khora_core::agent::registration`; one folder per family in
  `khora-lanes/src/<family>/` (`family.rs`, lanes, `registration.rs`; `overlay/{grid,wireframe,gizmo}/`);
  `khora-agents/src/` = `lane_agent/` + `registered_agent.rs`; residencies in `khora-data/src/residency/`;
  family tests in `khora-lanes/tests/it/<family>/` and `khora-agents/tests/it/families/<family>/`. Each
  rule is an xtask check (`xtask/tests/layout.rs`, `crate_graph.rs`).
- **Phase 7** (7a dead code `e19a4739` → 7h `9e0a56a0`): shims and dead items removed; the layout above;
  one primary type per file, in-file order, functions under 200 lines, large files split; public type
  names declared once (`CostBasis`, `LaneRun`, `PreparedLayer`, `StagedComponent`, `TrustLayer`, …);
  comments to the style guide (`conventions.md` §9b) crate by crate, what they lost moved to the book;
  shared helpers (`khora_io::fs`, `khora_core::sync::{LockExt, RwLockExt}`), atomic saves, the typed
  `EditorRequest` queue, `AssetType` with one extension table, measured per-frame costs.
- **Follow-ups after Phase 7** (`cc31b38d`): a packed game compiles its scripts from its pack
  (`ModuleSource`, `PackModules`); every start checks the pack's seal (`GameSeal::enforce` in
  `EngineBootstrap::begin_with`); a learnt correction says whether it was measured (`Correction.runs`);
  a row priced at no time is lent the frame's target; a file made at a renamed asset's old path gets a
  derived UUID of its own; File > Open Project is wired.
- **Conventions enforced by tools** (`0bcbcfcf`, plan `docs/plans/conventions-enforced.md`): workspace
  lints (`missing_docs`, clippy doc lints, `unwrap_used`, `print_*`, `undocumented_unsafe_blocks`),
  `clippy.toml` disallows `std::thread::spawn` and glam types, a `cargo doc -D warnings` step in the gate;
  `cargo xtask lint-file` runs the file-local checks, from a `PostToolUse` hook and the pre-commit hook;
  "Before you write" in `conventions.md`; every violation fixed in one pass.
- **8a** (`9b7834a2`): the book as an Astro Starlight site in `docs/site/` reading `docs/src`, rustdoc under
  `/api`, xtask `docs_site_nav_is_the_summary`. **8b** (`e430f94d`): the landing page as the site's index;
  its frames as a release asset (`npm run frames`). Detail: `reference/documentation.md`.

## Open
- The owner's to-do from 8b/8f: create the `landing-frames-v1` release (until then the site builds with
  its posters); set the repository's Pages source to "GitHub Actions" before the first deploy from `main`.
- `hidden_entities` is never filled: per-component activation is an ECS feature still to design (every
  Flow must honour the flag, or it is decorative).
- macOS: stamping a pack key breaks a binary's ad-hoc signature (re-sign after stamping).

## Earlier milestones (condensed; see git and `docs/plans/`)
- **2026-11 — agents/lanes contract, phases 1–6**: typed lanes and families, `LaneAgent`, GORNA as a
  protocol (envelopes, epochs, replay), lanes as layers, the arbiter (proposers behind a shield), the
  trust boundary (`khora-io/src/trust/`, signed packs). Decisions 22–25 in [`decisions.md`](./decisions.md).
- **2026-11-03 — frame governor**: `FrameReport` + governor replaced the PID; VRAM budget; DCC reads hardware.
- **2026-10-30 — CRPECS change detection**: world clock, column ticks, `Changed<T>`, `Mut<T>`; docs
  `docs/src/concepts/change-detection{,-formal}.md`.
- **2026-10-25 — unusable component values**: `#[component(check = …)]`, `try_*` math constructors, one
  list of shining lights (`render::shining_lights`).
- **2026-10-04 — persistence redesign**: records by name, `PersistentId`, atomic loads, game saves as
  three-way merges, prefab links, Ergon resume tiers. Detail: `reference/ecs-data.md`, `reference/scripting.md`.
- **2026-07 — editor**: dock (`DockTree`), scrolling, keyboard API, deterministic scene tree, modal
  confirmations, `ComponentProvenance`; stable asset identity (`.khora/asset-registry.ron`, decision 12).

## Architecture decisions
See [`decisions.md`](./decisions.md). 17 workspace members (14 `khora-*`, `sandbox`, `xtask`,
`workspace-hack`), plus `khora-macros` as a non-member path crate.
