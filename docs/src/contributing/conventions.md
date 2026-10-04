# Conventions

These are the rules a change has to follow to pass review and CI. This page is the
**human-readable summary**. The authoritative, machine-readable source — the
single source of truth that the project's AI agents also follow — lives in the
repository under `.agent/engine/` (`conventions.md` and `RULES.md`). When the two
ever disagree, those files win; this page is a digest, not a duplicate.

## Math

- All math goes through **`khora_core::math`** — `Vec2/3/4`, `Mat3/4`,
  `Quaternion`, `Aabb`, `LinearRgba`, and so on. **Never use raw `glam`.** If you
  need an operation the module doesn't expose, extend the module rather than
  reaching for the dependency directly.
- The convention is right-handed, column-major, **Y-up**. Document any non-trivial
  derivation with the formula or paper it comes from.

## Logging

- Log through **`log::{info, warn, error, debug, trace}`** — **never** `println!`
  or `eprintln!`. Those bypass the log pipeline and the editor console.
  - `info` for lifecycle events, `warn` for recoverable anomalies, `error` for
    unrecoverable errors before bubbling a `Result`, `debug`/`trace` for gated
    hot-path diagnostics.
- **One exception:** the engine's own log sink,
  `EditorLogCapture::log` (in `khora-editor/src/log_capture.rs`), writes to
  stderr with `eprintln!` — a `log::Log` implementation that called `log::*` would
  recurse forever. That is the only sanctioned `eprintln!` in the codebase.

## Errors

- **Never `unwrap()` on a fallible GPU or I/O operation.** One panic in render or
  asset code takes down the frame loop. Use `Result`, the `?` operator, or
  `map_err` to add context.
- Each subsystem owns its error enum (`LaneError`, `AssetError`, `PhysicsError`, …),
  derived with `thiserror`, with variants that carry context (paths, IDs, expected
  state). Validate at boundaries (user input, file I/O, GPU); trust internal
  contracts.

## `unsafe`

- Every `unsafe` block carries a `// SAFETY:` comment explaining why the invariant
  holds. No exceptions.

## Components and the ECS

- Use **`#[derive(Component)]`** on every ECS component. The macro generates the
  serializable mirror plus the `From` conversions, and self-registers the component
  via `inventory` — you don't wire it up by hand.
- Declare a component's domain with **`#[component(domain = Physics)]`** (or the
  relevant domain).
- Declare who writes it with **`#[component(provenance = …)]`** when it is not
  the author: `ToolAuthored`, `Derived` or `Runtime` (absent means `Authored`).
  Only `Authored` and `ToolAuthored` components are saved in a scene. Add
  **`resumable`** to `Runtime` state a game save must keep for play to resume
  (today only `ScriptState`) — a decision about how that domain reads it back,
  not a flag to set in passing.
- Use `#[component(skip)]` for fields that must not serialize (GPU handles,
  runtime caches). `#[component(no_serializable)]` drops the generated
  registration entirely: the component is never saved and never shown in the
  inspector, unless you submit a hand-written `ComponentRegistration` for it
  (as `MeshRef` and `MaterialRef` do).
- **Renaming is a declaration.** Saves hold components and fields by name, so
  rename a type with `#[component(formerly = "OldName")]` on the struct, and a
  field with `#[component(formerly = "old_name")]` on the field. **Removing** a
  component type a project may still have saved means declaring it retired —
  `inventory::submit! { khora_data::scene::RetiredComponent { name: "OldName" } }`.
  Anything else and loading a save that holds it fails with "unknown component
  type". Never write a payload migration.

## Agents

- An **agent implements only `Agent` and `Default`** — nothing else. No
  `start`/`stop`, no builders, no accessors; construction is via
  `Default::default()`. (Private free functions in the module file are fine.)
- An agent owns exactly **one `LaneKind`**, and its only jobs are: select a lane
  for the budget, negotiate that budget via GORNA, and dispatch
  `Lane::execute()`. No per-frame state, no buffering outputs, no owning a flow.
- Only use an agent for a subsystem that actually needs GORNA negotiation. For
  everything else use a direct service (`AssetService`, `SerializationService`, …).

## Lanes and flows

- **Lanes consume Views from the `LaneBus`** — they never query the World
  directly. The domain `Flow` (in `khora-data/src/flow/`) is the only legitimate
  producer of those Views, and a flow is a read-only projector: `select → project`,
  never a mutation.
- Never bypass the `Lane` abstraction for hot-path work.
- **Adapt the HOW, never the WHAT.** Automatic adaptation (DCC / GORNA / Flow /
  AGDF) may change representation — strategy, quality, memory layout — but must
  never change game semantics. Structural mutations are forbidden inside lanes.

## Shaders

- Shaders are **`.wgsl` files**, never inline Rust `const`/`static` strings. They
  live under `crates/khora-infra/src/graphics/shader/shaders/` — `pipelines/` for entry
  points, `lib/` for reusable modules — and are composed by the `PipelineSystem` backend with
  `naga_oil` `#import`. Write WGSL only (no GLSL or SPIR-V).
- **The four-bind-group budget.** Every render lane uses exactly four bind groups:
  `0` Frame (camera), `1` Object (per-draw model/normal matrix), `2` Material,
  `3` Lighting (the entire lighting domain). A new lighting feature adds *bindings
  to group 3*; it never adds a fifth group. Four groups is the universal wgpu
  baseline; bindings within a group are effectively unbounded.

## Architecture boundaries

- **Dependencies flow strictly downward**: `khora-core` → `khora-data` /
  `khora-control` → `khora-lanes` → `khora-agents` →
  `khora-sdk`. Never introduce a cycle. Abstract traits live in `khora-core`;
  concrete backends (wgpu, Rapier, CPAL, Taffy, winit) live under `khora-infra`.
- Keep GPU resources behind abstract IDs (`TextureId`, `BufferId`, `PipelineId`) —
  never expose raw wgpu handles in public APIs.
- Concurrency goes through the DCC agent system — **never `std::thread::spawn`
  directly** (test code may, for isolation).

## Before you push

- **`cargo gate` must be green.** It runs what CI checks, with CI's flags, and
  stops at the first failure: `cargo fmt` reporting no changes, clippy with
  `-D warnings`, the whole test suite (nextest), then the doc tests.
- While iterating, run the tests you are working on with
  `cargo t -p <crate> [filter]`: scoped to one crate, a run after a change
  relinks only that crate's tests.

See [Workflow](./workflow.md) for the exact CI gates.

---

Again: `.agent/engine/conventions.md` and `.agent/engine/RULES.md` in the repo are
the authoritative source. This page summarises them for humans — when in doubt,
read the source.
