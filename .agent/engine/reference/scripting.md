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
be told that. Ergon **suspends on an instruction boundary** and keeps the machine that suspended, so
the next frame resumes rather than restarts. Suspension is the normal path, not an error path.

## Key files
- Language: `crates/khora-script/src/` — `lexer/`, `parser/`, `ast/`, `bytecode/`, `vm/`, `types/`.
- Persistent state: `crates/khora-script/src/arena/` (`PersistentStore`, `Persisted`) — what survives a
  hot-reload, matched **by name** because a slot means nothing across a recompile.
- Value bridge: `crates/khora-script/src/bridge.rs` + `khora-core/src/script/table.rs` — the single
  X-macro table driving `ScriptValue` ↔ `Value` ↔ `Persisted` ↔ JSON.
- Hot-reload: `crates/khora-script/src/reload.rs`, `crates/khora-io/src/script_hot_reload.rs`.
- Compiling a module and everything it imports: `crates/khora-io/src/script_compile.rs`
  (`compile_module`, `DiskLoader`).
- Component mirrors: `crates/khora-io/src/script_mirror.rs` — turns each `ComponentShape`
  (`khora-data/src/scene/shape.rs`, emitted by `#[derive(Component)]`) into Ergon declarations, served
  at `engine/components.erg` by a `PreludeLoader` stacked over the `DiskLoader`.
- Lane: `crates/khora-lanes/src/script_lane/` — `mod.rs` (the `Lane`), `frame.rs` (the loop),
  `turn.rs` (one behavior's turn), `hooks.rs`, `runtime.rs` (`ScriptRuntime`), `reload.rs`,
  `report.rs`, `persistence.rs`.
- Agent: `crates/khora-agents/src/script_agent/mod.rs`.
- Engine→script events: `khora-core/src/script/event.rs` (`Channel<ScriptEvent>`).
- Script→engine effects: `khora-core/src/script/buffer.rs` (`CommandBuffer`, `WorldCommand`), applied
  by a `DataSystem` in `khora-data/src/ecs/systems/script_commands/`.

## Hard rules
- **A script never writes the `World`.** Its effects are queued as `WorldCommand`s and applied by a
  data system during `Maintenance`. This is what lets the script lane run world-free.
- **The agent chooses a lane; the lane does the work.** `ScriptingAgent` holds lanes, current lane,
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
  `script_mirror.rs`; that test is what will say the projected read has landed.
- `ENGINE_TYPES` (`khora-script/src/types/ty.rs`) is a list of names with a `Value` variant, a
  constructor and accessors behind them. It once named `Transform`, which had none, so `Transform t;`
  shaped cleanly and failed with the misleading "`Transform` has no `x`". Adding a name there without
  the three is worse than leaving it out — the mirrors give a component its fields through an ordinary
  `struct` declaration instead.

## Skills
- [`build-and-test`](../skills/build-and-test/SKILL.md) · [`debug-frame`](../skills/debug-frame/SKILL.md)
  · [`run-the-engine`](../skills/run-the-engine/SKILL.md).
