# Scripting (Ergon) — reference

Domain knowledge for **Ergon**, Khora's gameplay language. Consulted during Research (dispatch the
`codebase-*` subagents to apply it to concrete files). Follow [`../RULES.md`](../RULES.md) §8.

## Scope
The language and its VM: lexing, parsing, AST, bytecode, the interpreter, the persistent field arena,
hot-reload, native functions, and the bridge between script values and engine types. Also the lane and
agent that run it inside a frame budget.

## Why it exists

The DCC must be able to say *"you have 0.4 ms, hand back control"*. Every other subsystem already
degrades under pressure; gameplay was the one that could not, because no off-the-shelf language could
be told that. Ergon **suspends at named safepoints** — a statement's start, a loop's head (every back
edge lands on one), a function's entry, just after a call returns, after an `await` — and keeps the
machine that suspended, so the next frame resumes rather than restarts. Past its fuel a run continues
only to the next safepoint; that overdraft is bounded by `Program::max_overdraft` (the costliest acyclic
stretch between two checks, computed at seal time in `bytecode/fingerprint.rs`). Suspension is the
normal path, not an error path. Scripts run **only in `EngineMode::Playing`**.

## Key files
- Language: `crates/khora-script/src/` — `lexer/`, `parser/`, `ast/`, `bytecode/`, `vm/`, `types/`.
- Persistent state: `crates/khora-script/src/arena/` (`PersistentStore`, `Persisted`) — what survives a
  hot-reload, matched **by name** because a slot means nothing across a recompile.
- Value bridge: `crates/khora-script/src/bridge/mod.rs` + `khora-core/src/script/table.rs` — the single
  X-macro table driving `ScriptValue` ↔ `Value` ↔ `Persisted` ↔ JSON.
- Hot-reload: `crates/khora-script/src/reload.rs`, `crates/khora-io/src/script/hot_reload.rs`.
- Compiling a module and everything it imports: `crates/khora-script/src/pipeline.rs`
  (`compile_module`, `CompileOutcome`), reading through a `SourceLoader`; the disk one is
  `DiskLoader` in `crates/khora-io/src/script/compile.rs`. Import paths and the `.erg`
  `EXTENSION`: `crates/khora-script/src/modules/`.
- Component mirrors: `crates/khora-io/src/script/mirror.rs` — turns each `ComponentShape`
  (`khora-data/src/scene/shape.rs`, emitted by `#[derive(Component)]`) into Ergon declarations, served
  at `engine/components.erg` by a `PreludeLoader` stacked over the `DiskLoader`.
- Lane: `crates/khora-lanes/src/script_lane/` — `mod.rs` (the `Lane`), `frame.rs` (the loop),
  `turn.rs` (one behavior's turn), `hooks.rs`, `runtime.rs` (`ScriptRuntime`), `reload.rs`,
  `report.rs`, `persistence.rs`.
- Lane resume / saves: `script_lane/resumption.rs` (load and hot reload share the VM tiers),
  `persistence.rs` (snapshot ↔ instance), `frame.rs` (arrivals, `OnLoad`, `OnResumeFailed` owed).
- Agent: `crates/khora-agents/src/script_agent/agent.rs`.
- Lifecycle hooks table: `crates/khora-script/src/lifecycle.rs` (`Update`, `OnSpawn`, `OnDespawn`,
  `OnLoad`, `OnResumeFailed(string member)`, reserved `FixedUpdate`).
- Safepoints and sites: `bytecode/sites.rs` (site grammar, `timer_names`), `bytecode/keys.rs` (statement
  keys), `bytecode/stmt.rs` (emits `Safepoint`), `vm/site.rs` (`Site`, `SiteKind`).
- Fingerprints + overdraft: `bytecode/fingerprint.rs`; `vm/program.rs` (`Program::fingerprint`,
  `max_overdraft`).
- Freeze / resume: `vm/freeze.rs`, `vm/resume.rs` (`resume`, `ResumeTier`, `Abandoned`); the engine-side
  form `khora-core/src/script/frozen.rs` (`FrozenMachine`, `FrozenFrame`, `FrozenLocal`, `FrozenValue`).
- What a save holds: `khora-core/src/script/snapshot.rs` (`ScriptSnapshot`, `InstanceLifecycle`,
  `RecordedFault`), the component `khora-data/src/ecs/components/script_state.rs` (`ScriptState`:
  `provenance = Runtime, resumable`), written back by `khora-data/src/ecs/systems/script_state/` from
  `ScriptStateWriteback` (`khora-core/src/script/writeback.rs`).
- Mode gate: `khora-data/src/flow/script.rs` (`gameplay_runs` → `EngineMode::runs_gameplay`) and the
  `register_agent_for_mode(…, vec![EngineMode::Playing])` call in `khora-sdk/src/engine/bootstrap.rs`.
- Conformance suite: `crates/khora-script/tests/it/conformance.rs` (`SHIPPED` / `PENDING` rows); resume
  suites beside it (`safepoints.rs`, `sites.rs`, `fingerprints.rs`, `freeze.rs`, `rebuilt_frames.rs`,
  `resume_tiers.rs`).
- Engine→script events: `khora-core/src/script/event.rs` (`Channel<ScriptEvent>`).
- Script→engine effects: `khora-core/src/script/buffer.rs` (`CommandBuffer`, `WorldCommand`), applied
  by a `DataSystem` in `khora-data/src/ecs/systems/script_commands/`.

## Hard rules
- **A script never writes the `World`.** Its effects are queued as `WorldCommand`s and applied by a
  data system during `Maintenance`. This is what lets the script lane run world-free.
- **The agent chooses a lane; the lane does the work.** `ScriptAgent` holds lanes, current lane,
  strategy, fuel and a handle to the shared `ScriptRuntime` — nothing else. Applying reloads, draining
  channels, delivering events and keeping counters all live in the lane (`RULES.md` §8).
- **`ScriptRuntime` lives in `Runtime::services`** as `Arc<Mutex<ScriptRuntime>>`, like the physics
  provider. The agent locks it for the duration of `execute` and declares it in `contention()`.
- **An event is never delivered in the frame it was raised.** Handling one where it was raised opens a
  cascade with no bound, and a budget that cannot bound the work is not a budget. Script-to-script mail
  waits on `ScriptRuntime`; engine-raised events arrive through `Channel<ScriptEvent>`.
- **Degrading defers whole behaviors, it does not thin every behavior.** A half-run behavior decides
  from a partial read of the world; a deferred one acts a frame late, which gameplay absorbs.
- **Adding a type to the language touches two places, not eight** — a row in the `script_value_table!`
  X-macro (`khora-core/src/script/table.rs`) and a line of the `ergon_type!` declaration
  (`khora-script/src/native/engine_types.rs`). It is a declaration, not an attribute, and deliberately:
  a `ScriptType` needs a matching `Value` variant, which only the table can supply, so a game cannot
  expose a type of its own this way whatever the syntax. If a change needs a new `match` arm in four
  files, the table is being bypassed.
- Fields carried across a hot-reload are matched **by name**. A positional carry-over silently moves one
  guard's health into another's ammo.
- **The component mirrors are served, never written to disk.** A generated file an author can open goes
  stale and is then edited, in that order, and the second is found long after the first. `PreludeLoader`
  answers `engine/components.erg` ahead of the inner loader, so a file left at that path — a copy of an
  older engine — cannot shadow the mirror of the one running.
- **A mirror that cannot express something says so; it never guesses.** A field whose Rust type Ergon
  has no spelling for stays as a comment naming that type; a component whose every field is like that,
  or whose field names the language cannot spell (a tuple index, or a reserved word — `Script.behavior`,
  `UiInteraction.state`), is left undeclared with the reason. An empty `struct` means *marker*, and
  emitting one for a component that does carry data would be a false statement.

## Resume (tiers)
One path for every resume — next frame, hot reload, game load (`vm/resume.rs::resume`):
1. **Exact** — same program fingerprint; 2. **Unchanged** — every function on the stack has its frozen
fingerprint; 3. **Rebuilt** — every frame's site still exists, locals matched by name + type, the same
number of temporaries; 4. **Restarted** — the member still takes the same parameters: rerun from entry
with the original arguments (side effects may repeat); 5. else **`Abandoned`** — the instance is owed
`OnResumeFailed(string member)` on its next turn (member like `"OnSpawn"`, `"Patrol.OnHit"`,
`"__every(0.5)"`).
- **Site grammar** (`bytecode/sites.rs`): `site := "entry" | path [":" point]`,
  `step := kind "." hash8 ["#" n]`, `branch := then|else|body`, `point := head | await[#n] | call.callee[#n]`.
  Named from what statements *say*, never from a counter, so an edit elsewhere renames nothing.
- **Keys are exhaustive** (`bytecode/keys.rs`): a compound statement is keyed by its header only. A new
  `Stmt`/`Expr` node must decide what it contributes, or two statements share a key.
- **The fingerprint normalizer is exhaustive** (`bytecode/fingerprint.rs`): an operand naming something
  outside the function (callee, native, literal, field) is hashed by name, never by index.
- **Timers** are functions named `Owner.__every(0.5)` / `Owner.__after(2)`, `#n` for identical siblings
  (omitted when zero) — the identity their countdown is saved under.
- **`FrozenMachine`** (engine terms, every encoding can read it): frames **name** their function, literals
  are kept as text; each `FrozenFrame` carries its `site`, function `fingerprint`, `locals` (name, type,
  scope, register) and `temporaries`; the machine carries the body's original `arguments`. Positions
  inside a function stay numbers, guarded by fingerprints.

## Saves and lifecycle
- `ScriptState` is `Runtime` + `resumable`: never in a scene, always in a game save — a guard saved
  mid-attack loads mid-attack.
- `InstanceLifecycle { spawned, fault }` is **recorded**, not derived: an empty snapshot does not say
  whether `OnSpawn` ran. A fault recorded under another program fingerprint is cleared on load.
- `OnLoad` runs once, only when an instance is **restored from a save** — after the initialiser and
  restore, before anything else that turn. A scene load or editor Stop is a fresh start (`OnSpawn`).

## Traps
- `khora-script` depends on `khora-core` and `khora-macros` **only**, deliberately: the compiler and VM
  stay testable without booting an engine. Do not reach for `khora-data` from here.
- The rate that converts a time budget into fuel is measured by the **lane** and stored on
  `ScriptRuntime`, not assumed by the agent. `INITIAL_RATE` lives in
  `khora-lanes/src/script_lane/runtime.rs` and is the single definition.
- **An Ergon `struct` type-checks but its fields do not lower.** `compile_field`
  (`khora-script/src/bytecode/expr.rs`) emits an accessor call for `Shape::Engine` and refuses
  everything else with "only an engine type's components can be read yet". This predates the mirrors and
  applies to every `struct`, including one a game declares — so a mirrored `t.translation` passes the
  checker and stops at the compiler. Pinned by `reading_a_mirrored_field_still_needs_the_ecs_bridge` in
  `khora-io/src/script/mirror.rs`; that test is what will say the projected read has landed.
- `ENGINE_TYPES` (`khora-script/src/types/ty.rs`) is a list of names with a `Value` variant, a
  constructor and accessors behind them. It once named `Transform`, which had none, so `Transform t;`
  shaped cleanly and failed with the misleading "`Transform` has no `x`". Adding a name there without
  the three is worse than leaving it out — the mirrors give a component its fields through an ordinary
  `struct` declaration instead.

## Skills
- [`build-and-test`](../skills/build-and-test/SKILL.md) · [`debug-frame`](../skills/debug-frame/SKILL.md)
  · [`run-the-engine`](../skills/run-the-engine/SKILL.md).
