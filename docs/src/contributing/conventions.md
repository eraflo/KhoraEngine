# Conventions

<p class="kp-lede">A workshop where everyone puts the tools back on the same hook finds them again without asking. These are Khora's hooks: where a new piece goes, how it is named, how it is commented, and what has to be green before it is pushed.</p>

These are the rules a change has to follow to pass review and CI. This page is the
**human-readable summary**. The authoritative source, the one the project's AI
agents also follow, lives in the repository under `.agent/engine/`
(`conventions.md` and `RULES.md`). When the two disagree, those files win; this
page is a digest, not a duplicate.

## Before you write

Decide where things go before writing any code, test or doc:

1. Name the file and folder of every new type and module first: one primary type
   per file, beside its siblings, never in a generic util file.
2. Look for the shared record, widget or helper before making one (the codegraph
   first); reuse it.
3. Keep dependencies pointing down: a shared widget never imports a panel, a
   generic module never a domain one.
4. Rename what you change the meaning of (a function, a field, a file); check a
   new name is not taken elsewhere.
5. Order each file: licence header, `//!` doc, imports, constants, primary type,
   its impls, private helpers, tests.
6. Never put an item between a type and its `impl`s; public functions before
   private ones; test helpers before the tests.
7. Put each `use` in its group: `std`, then external and `khora_*` crates, then
   `crate::` / `super::`.
8. Write a doc as the contract: a one-sentence summary (a function's starts with
   a verb, about 15 words), never the mechanism.
9. Never tell history in a comment or doc ("now", "used to", "always was"):
   state what holds.
10. List every error and panic in `# Errors` / `# Panics`, and every side effect
    (a file written, play ended) in the doc.
11. Reread the docs of what you changed: no parameter that is gone.
12. Keep one home per fact: name the `LABEL` const, never repeat its string; a
    design goes to the book.
13. Give every new public function a unit test (`#[cfg(test)]`), not only an
    integration test.
14. Put a test in the test file of the module it pins, named by the behaviour it
    pins.
15. Add every new public `khora_core` item to the public-paths guard
    (`crates/khora-core/tests/public_paths/`).
16. Update the book (`docs/src/`) in the same change when a name or behaviour it
    describes changes.

Before reporting a change, run both and leave them clean:

```sh
cargo xtask lint-file <every .rs file touched>
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items
```

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
- Each subsystem owns its error enum (`LaneError`, `RenderError`,
  `SerializationServiceError`, …), derived with `thiserror`, with variants that
  carry context (paths, IDs, expected state). Validate at boundaries (user input,
  file I/O, GPU); trust internal contracts.

## `unsafe`

- Every `unsafe` block carries a `// SAFETY:` comment explaining why the invariant
  holds. No exceptions.

## Components and the ECS

- Use **`#[derive(Component)]`** on every ECS component. The macro generates the
  serializable mirror plus the `From` conversions, and self-registers the component
  via `inventory` — you don't wire it up by hand. A generic type the derive does
  not cover (an asset reference, `AssetRef<A>`) implements `Component` by hand,
  and each kind of it is registered once (`register_asset_ref!` in
  `khora-data/src/ecs/components/asset_ref/registered.rs`).
- Declare a component's domain with **`#[component(domain = Physics)]`** (or the
  relevant domain).
- Declare who writes it with **`#[component(provenance = …)]`** when it is not
  the author: `ToolAuthored`, `Derived` or `Runtime` (absent means `Authored`).
  Only `Authored` and `ToolAuthored` components are saved in a scene. Add
  **`resumable`** to `Runtime` state a game save must keep for play to resume
  (no Rust component is; a behavior's state companion is) — a decision about how
  that domain reads it back, not a flag to set in passing.
- Use `#[component(skip)]` for fields that must not serialize (GPU handles,
  runtime caches). `#[component(no_serializable)]` drops the generated
  registration entirely: the component is never saved, never shown in the
  inspector, never reachable by name. It is not a synonym for runtime state;
  choose a provenance for that.
- A component that must be watched is declared by its observer (the `observes`
  list of a DataSystem or Flow, read with `Changed<T>`), never polled.
- **Renaming is a declaration.** Saves hold components and fields by name, so
  rename a type with `#[component(formerly = "OldName")]` on the struct, and a
  field with `#[component(formerly = "old_name")]` on the field. **Removing** a
  component type a project may still have saved means declaring it retired —
  `inventory::submit! { khora_data::scene::RetiredComponent { name: "OldName" } }`.
  Anything else and loading a save that holds it fails with "unknown component
  type". Never write a payload migration.

## Families, agents and lanes

- **Never write a new agent type.** A new subsystem is a **family**: its lanes
  (each implementing `khora_core::lane::Lane`), a `LaneFamily`, and one
  `register_agent!` line in its `registration.rs` (phase, importance, `after`,
  fixed step, default level, lanes). Its agent is always the one `LaneAgent` of
  `khora-agents`, built from that registration; the xtask check
  `only_lane_agent_implements_agent` keeps it the only one. See
  [Add a subsystem (a family)](../how-to/add-a-family.md).
- An agent runs exactly **one family**, and its only jobs are: offer the family's
  rows to GORNA, choose the row that fits the envelope it is lent, and install its
  layers at the family's place. It has no `execute`: the scheduler runs the
  installed layer every frame and wakes the agent only for an event. No per-frame
  state, no buffering outputs, no owning a flow, no reaching the World.
- A family never offers one `StrategyId` twice; boot refuses it.
- Only make a family for a subsystem that actually needs GORNA negotiation. For
  everything else use a direct service (`AssetService`, `SerializationService`, …).
- What an agent competes for, its `Contention`, is derived from its lanes' typed
  inputs and outputs; nobody writes it by hand. Never reach
  `context.runtime.{resources, services, backends}` from a lane: go through its
  typed inputs.
- An asset's copy on a device is a **residency** (`register_residency!` in
  `khora-data/src/residency/`), never a lane's state.

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

- **Dependencies flow strictly downward**, tier by tier; the
  [Crate map](../reference/crates.md#the-tiers) lists the tiers and what each crate
  may use, and `xtask/tests/crate_graph.rs` checks them. Never introduce a cycle.
  Abstract traits live in `khora-core`; concrete backends (wgpu, Rapier, CPAL,
  Taffy, winit) live under `khora-infra`, which uses `khora-core` only.
- Keep GPU resources behind abstract IDs (`TextureId`, `BufferId`, `PipelineId`) —
  never expose raw wgpu handles in public APIs.
- Concurrency goes through the DCC and the scheduler — **never
  `std::thread::spawn` directly** (test code may, for isolation).
- Never add an optional feature to a workspace crate, and declare a third-party
  dependency once, with every feature it needs: either would build the engine
  twice. After changing a dependency, run `cargo hakari generate` then
  `cargo hakari manage-deps`.

## Comments

**Code says how; comments say what and why.**

1. **Doc comment (`///`) = the contract.** First paragraph: *one sentence*, at most 2 lines (about
   15 words). A function's summary starts with a verb ("Returns…", "Spawns…"); a type's is a noun
   phrase. Then, if needed, one short paragraph of what the signature cannot say: preconditions,
   side effects, ordering ("call after…"), units, invariants, locks held. Then the standard
   sections: `# Errors`, `# Panics`, `# Safety`, `# Examples`. Aim for 6 prose lines or fewer;
   more only for a public entry point with an example.
2. **Never the implementation in a doc.** No step lists, no "the scheduler then…". If the reader
   needs the mechanism, it is a book page.
3. **Every `pub` item is documented.** A private item gets a doc only when its name and
   signature leave something non-obvious, usually in one line.
4. **Inline comment (`//`) = why.** A reason, a trap, an invariant relied on. Four lines at most,
   full sentences. No numbered steps: a function with a step list is split into named functions.
5. **Never in a comment**: history ("previously", "replaces", "no longer"), plan or review labels
   (`Phase N`, `step 3f`, `P1`, `R7`), restating the code, rules copied from `RULES.md`, design
   essays. History goes in the commit message.
6. **One home per fact.** Design, rationale, frame order and protocols live in `docs/src`. A
   comment points there with `See the book: concepts/the-frame.md, "Heading".` (a path and a
   heading, both checked).
7. **`//!` module doc**: 1–3 lines saying what the module holds. A crate's `lib.rs` may say more.
8. **`// SAFETY:`** on every `unsafe` block, unchanged. The 13-line licence header stays.

Checked by `cargo xtask lint-file` and `xtask/tests/layout.rs`:
`no_plan_labels_in_comments`, `book_citations_resolve`, `doc_summary_is_short`,
`inline_comments_are_short`, `comments_tell_no_history`,
`fn_summaries_start_with_a_verb`, `a_type_and_its_impls_are_contiguous`,
`every_public_fn_is_named_by_a_test`; and by the workspace lints (`missing_docs`,
clippy's doc lints, `undocumented_unsafe_blocks`) and the `cargo doc` step of
`cargo gate`.

**Checklist before commit**
- Each summary is one sentence and fits in 2 lines.
- Errors, panics and unsafety are in their sections, not in prose.
- No doc describes how the body works; no comment restates a line.
- No plan label, no history, no step numbering.
- Anything longer than a paragraph is in the book, linked by path and heading.

## The book

- A page opens with the idea in everyday words (`<p class="kp-lede">`), then goes
  down to how it works; the maths goes in its advanced part.
- Mechanisms are explained with hand-made SVG figures under
  `docs/src/images/<topic>/`, never Mermaid; what unfolds over time gets a
  step-through (`<figure class="kp-steps">`). The `docs_figures` test checks every
  figure: a root `id` starting `fig-`, `role="img"`, a `<title>` named by
  `aria-labelledby`, every style selector scoped to that id, `kp-` classes, no
  blank line, no id shared by two figures.
- Every page is listed once in `docs/src/SUMMARY.md` (`docs_pages_are_in_the_summary`).

## The hooks

Two checks run without being asked, once the AI installer has set them up
(`node .agent/engine/installer/bin/khora-ai.mjs install all`):

| Hook | When | What it does |
|---|---|---|
| `lint-file` | after an agent writes a file in a session, and at `git commit` on the staged `.rs` files | runs `cargo xtask lint-file`: in-file order, file and function size, comments (plan labels, history, summary length and verb, book citations) and the crate boundaries one file shows. A finding sends the agent back to fix it, or blocks the commit. |
| `secret-scan` | at `git commit` | scans the staged files for private keys, tokens and passwords, and blocks the commit on a match. A confirmed false positive is allowed inline with `# khora-ai:allow-secret`. |

`lint-file` skips a file that is not `.rs` or lies outside `crates/`, `xtask/`
and `examples/`. What one file cannot show (whether a public function is named
by a test, the crate graph, orphan files) stays in `cargo t -p xtask`.

## Before you push

- **`cargo gate` must be green.** It runs what CI checks, with CI's flags, and
  stops at the first failure: `cargo fmt` reporting no changes, clippy with
  `-D warnings`, the whole test suite (nextest), then the doc tests.
- While iterating, run the tests you are working on with
  `cargo t -p <crate> [filter]`: scoped to one crate, a run after a change
  relinks only that crate's tests.

See [Workflow](./workflow.md) for the exact CI gates.

---

`.agent/engine/conventions.md` and `.agent/engine/RULES.md` in the repo are the
authoritative source. This page summarises them for humans — when in doubt, read
the source.
