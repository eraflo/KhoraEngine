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
- Control flow: `bytecode/flow.rs` — `LoopContext` (`break`/`continue` from anywhere in a loop),
  `if`/`while` with their `var` narrowing forms, `match`, all on `JumpIfNot` / `JumpIfNull`. Behavior
  compilation (layout, members, states, field defaults): `bytecode/behavior.rs`. Return-path check:
  `types/returns.rs`.
- Safepoints and sites: `bytecode/sites.rs` (site grammar, `timer_names`), `bytecode/keys.rs` (statement
  keys), `bytecode/stmt.rs` (emits `Safepoint`), `vm/site.rs` (`Site`, `SiteKind`).
- Fingerprints + overdraft: `bytecode/fingerprint.rs`; `vm/program.rs` (`Program::fingerprint`,
  `max_overdraft`).
- Freeze / resume: `vm/freeze.rs`, `vm/resume.rs` (`resume`, `ResumeTier`, `Abandoned`); the engine-side
  form `khora-core/src/script/frozen.rs` (`FrozenMachine`, `FrozenFrame`, `FrozenLocal`, `FrozenValue`).
- What a save holds: `khora-core/src/script/snapshot.rs` (`ScriptSnapshot`, `InstanceLifecycle`,
  `RecordedFault`), carried by a behavior's state companion `Guard#state` (registered `Runtime` +
  resumable by `khora-io/src/script/declarations.rs`; its fields `khora-data/src/flow/companion/`),
  written back by `khora-data/src/ecs/systems/behavior_writeback/` from `BehaviorWriteback`
  (`khora-core/src/script/writeback.rs`).
- Mode gate: `khora-data/src/flow/script.rs` (`gameplay_runs` → `EngineMode::runs_gameplay`) and the
  `register_agent_for_mode(…, vec![EngineMode::Playing])` call in `khora-sdk/src/engine/bootstrap.rs`.
- Conformance suite: `crates/khora-script/tests/it/conformance.rs` (`SHIPPED` / `PENDING` rows); resume
  suites beside it (`safepoints.rs`, `sites.rs`, `fingerprints.rs`, `freeze.rs`, `rebuilt_frames.rs`,
  `resume_tiers.rs`).
- Engine→script events: `khora-core/src/script/event.rs` (`Channel<ScriptEvent>`).
- Script→engine effects: `khora-core/src/script/buffer.rs` (`CommandBuffer`, `WorldCommand`), applied
  by a `DataSystem` in `khora-data/src/ecs/systems/script_commands/`. **Column writes (E3):**
  `WorldCommand::WriteColumn { component, batch: Arc<ColumnBatch> }` (`script/batch.rs`, fields private,
  rows checked for width) rides the ordered stream; the applier resolves the component and fields once
  and calls `World::write_column` (`ecs/world/column_write.rs`): rows placed through the domain's entity
  locations, stably sorted by page and row, `ColumnOps::write_rows` per page (Rust `Vec<T>` written in
  place, a refused row put back field by field), one epoch bump. Skipped rows are counted
  (`apply_all_reporting` → `ApplyReport`). `Parent`/`Children` are refused in a batch.
  **Conflicts are per field, two levels** (`for_each_write_target`, borrowed `WriteTargetRef`): a whole
  write (`Add`, `Remove`, a non-patch `Set`) conflicts with any write to that component; a patch or a
  column cell only with the same field.

## Hard rules
- **A system is a behavior with no subject — never a second runtime.** It runs as an instance on its
  singleton entity; anything that needs an entity of its own is refused in it. `ref` is a system's only;
  `ref` on a behavior's component is refused (a turn builds on earlier `Set`s, not on column writes).
- **A `ref` batch never outlives its run** and leaves before a later command on its component — the run's
  writes keep their order. A sweep's rows are taken once; structural changes in the body are commands.
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
- **Text never outlives its frame by reference.** The frame arena (`arena/mod.rs`) is owned by
  `ScriptRuntime` and lent to each frame's `Host` (`Host::with_arena` / `take_arena`), so its generation
  only grows and a stale `ArenaRef` always faults (`BadString`) — a fresh arena per frame would restart
  the count and let it read another frame's text. A suspending machine **evacuates** live arena text into
  `Machine.held` (`StrRef::Held`) and **rehydrates** it on the next run (`vm/held.rs`); it freezes as
  `FrozenValue::Text`. Fields and state data keep text by value (`Machine::persist`, for `StoreField`
  and `Become`).
- **`null` is a saved value.** `ScriptValue::Null` crosses into a save and back (`int? best = null`
  loads as `null`); `from_register` and `deliver` refuse it, so an event never carries one. An unwritten
  slot is `Value::Unit`, not `Null`.
- **A value carried by name must fit its declared type.** Layouts record each field's and state slot's
  written type (`BehaviorLayout::field_types`, `StateLayout::types`); `persistence::set` drops a carried
  value `bridge::fits` refuses (a `null` into a retyped `int`), so the field takes its default.
- **Arrays and structs are values** (`Value::Obj(ObjRef)`, `Object::{Array, Struct}`): the compiler copies
  (`Copy`) a value read from a place when it is bound — `bytecode/objects.rs::compile_value` — and writes
  through a path in place, evaluating indices and the right-hand side *first*, so no write goes through a
  reference taken before a stop. A path rooted at a field ends with `WriteBack`, never a stop.
- **One owned form** (`arena/owned.rs::Owned`, `Arena::export`/`import`): the store (`Persisted::Owned`), a
  suspended machine (`Machine.held`) and the bridge all use it; a struct is owned **by field names** and
  imported against `Program::structs` (`StructLayout`, constant `defaults`), so an edit adding or removing a
  field keeps the rest. `ScriptValue::Struct` carries no type name: `bridge::to_persisted_as` takes the
  declared type (`BehaviorLayout::structs`).
- **Sized operations** (`vm/objects.rs::sized_cost`): `Copy`, `LoadField`/`StoreField` of an object are
  charged their size and are checkpoints (the VM stops before one it cannot pay for, unless first in the
  run; `SiteKind::Checkpoint` names the place). `WriteBack` and `Become` are charged but never stopped at —
  a state entry or an update is never half-done.
- **The checker's types reach the compiler** through `Checked::types` (every expression's `Ty`, keyed by
  address), as `inferred` does for `var`.
- **Calls resolve lexically.** Inside state `S`: `S`'s methods, then the behavior's, then free functions
  and natives (`types/members.rs`, `bytecode/expr.rs::compile_call` via `Compiler.state`). A state's
  method called from outside it is a checker error naming the state.
- **The component mirrors are served, never written to disk.** A generated file an author can open goes
  stale and is then edited, in that order, and the second is found long after the first. `PreludeLoader`
  answers `engine/components.erg` ahead of the inner loader, so a file left at that path — a copy of an
  older engine — cannot shadow the mirror of the one running.
- **Entity operations are methods; one spelling.** A native whose first parameter is `Entity` is called
  `e.Name(…)` and compiles to the same `NativeCall`, receiver first (`types/components.rs`,
  `bytecode/components.rs`); the free form is refused with "Write `e.Name(…)`". `Set`/`Add`/`Remove`
  (and `Get`/`Has`, 07) are intrinsics; with `Spawn` they are reserved names (`types/components.rs::RESERVED`).
- **A component write is a patch.** `e.Set(C { … })`, `e.Add(C { … })`, `e.Remove(C)`, `Spawn(pos, C { … }, …)`
  compile to `Instruction::{WriteComponent, RemoveComponent, SpawnEntity}` over `Program::{patches, spawns}`
  (component + field *names*, written order) and queue `WorldCommand::{SetComponent, AddComponent,
  RemoveComponent, Spawn}` in `host.commands` (`vm/commands.rs`); cost `1 +` fields; never a stop. A
  component literal is valid only as their argument; a component is not a type.
- **Component reads: a per-frame projection, a working set.** `e.Get(C)` → `C?` (a read-only copy:
  `types/components.rs::refuse_copy_write`), `e.Has(C)` → bool; `Instruction::{GetComponent, HasComponent}`
  (`vm/commands.rs`) answer from `Host::components: khora_core::script::FrameComponents` — the view's
  `ComponentProjection` with this frame's own `Set`/`Add`/`Remove` observed over it as they are queued.
  `Program::reads` (sorted) → the lane publishes the union to `Arc<ScriptCatalog>` every frame
  (`script_lane/reload.rs`); `ScriptFlow` projects only those (`World::project_components`, one
  `ColumnSnapshot` per page column, live rows only — snapshots include orphan rows). A reload whose reads
  were not projected this frame is deferred one frame. A `component` declaration is also a `StructLayout`, so
  a copy imports as a struct.
- **Behaviors run as components.** Registry: `ComponentVTable.script: Option<ScriptRole{Behavior{module, order}|
  System{module, order}|BehaviorState{behavior}}>` and `declared_by` (the module declaring it now; `None` for an orphan or a
  file-only declaration), set by `apply_declarations`. Before an app's `setup`, the declarations of the
  scripts mounted at startup are registered (`register_pending`), so `GameWorld::add_declared_component`
  attaches a behavior by name. Flow (`flow/behaviors.rs`, `World::behavior_components`): behaviors and
  systems by `(module, order, name)` — `order` = source position among the module's behaviors and
  systems (`DeclaredComponent.order`) — rows by entity, `ScriptInstance.row` = the full row every
  frame, `arrival` only for a new `(entity, behavior)` (fields away from the registered defaults, or — restored — the row and the
  companion as `observed`). Lane (`script_lane/attached.rs`, `shares.rs`): `seen` rows → fields written
  outside are adopted; `BehaviorUpdate.changed` = fields the turn changed; fuel shared per behavior (equal
  shares among those that can spend, rotating remainder, debt for overdraft, a cursor each); an arriving
  instance takes its fields from its row and its state from its companion — no base/override merge (the
  game save's field-by-field merge carries a scene edit to a field the game left alone); hot reload carries
  from the layout an instance was built for (`Instance.built_for`). Removing a behavior's component
  farewells it (`OnDespawn`) and drops its companion. Writeback (`systems/behavior_writeback/`): changed
  fields as `World::write_column` per `(behavior, fields)` — a value a field refuses is left out — and the
  companion whole (`flow/companion` ↔ `ScriptSnapshot`). `collision_to_script` tells every holder of a
  behavior component. The legacy `Script`/`ScriptState` components are retired (`scene/retired.rs`): an old
  file loads with them skipped and reported, and a script may not declare either name.
- **Behaviors are components — registered and persisted.** `modules/behaviors.rs::declared_behaviors`: each project behavior declares `Guard`
  (`DeclaredKind::Behavior`, its fields in `[Domain(…)]`, default = the constant, else the zero, else a
  constant — defaults are layout data, `BehaviorLayout.defaults`) and `Guard#state` (`DeclaredKind::BehaviorState`,
  Script domain, fixed fields `state, state_fields, timers, pending, spawned, fault,
  resume_failed`; `<any>` = `ErgonType::Any`). Registration: the companion is `Runtime` + `resumable`
  (`ComponentVTable.resumable`); a behavior refused or unregistered takes its companion with it.
  Persistence: `is_saved_declared(vtable, kept)` — a scene and the Play snapshot keep `Guard`, a game save
  both, `promote` never the companion; `DeclaredRecord.resumable`; a scene load reports a companion
  `NotSaved` and registers nothing for it. Names are global (a behavior named like any component is
  refused). Unknown behavior attributes are refused.
- **Script-defined components (spec 08).** A `component` outside `ENGINE_COMPONENTS_MODULE` is the
  project's: `[Domain(D)]` (one, nothing else), field defaults (constants; an engine value of numbers
  `Vec3(…)` too — component fields only), zero when absent, a type with no zero needs one. Checked per module
  before the merge (`modules/components.rs::report_script_components`): a name in
  `SourceLoader::engine_components` (every registration name + `formerly`, from `PreludeLoader`) is refused at
  its span; duplicate fields, `Map` fields and non-finite literals too. `Program.declared:
  Vec<DeclaredComponent{name, module, domain, fields: {name, ty: ErgonType, default}}>` (sorted). The compile
  path (`load_all`, the pump) sends one `ScriptDeclarations` per declaring module on
  `Channel<ScriptDeclarations>`; the `script_components` DataSystem (PreExtract, after `script_hot_reload`,
  **every mode**) runs `khora_io::script::declarations::apply_declarations`: register, relayout (a type change
  the storage kind hides is reset via `relayout_resetting`), `move_component_domain`, orphan (never delete),
  adopt; a second module declaring an owned name is refused (`report.refused`). The editor does not call
  `scripts::mount` yet — it learns project components only from the scenes it loads.
- **The engine's components come from `ENGINE_COMPONENTS_MODULE`** (`engine/components.erg`). The mirror
  emits `component` for hand-authorable provenance except `Transform` (placed by
  `native::world::PLACEMENT`), and `struct` with a `//` reason for the rest.
- **A mirror that cannot express something says so; it never guesses.** A field's Ergon type comes from
  `ScriptField` (`ComponentRegistration::script_type`), never from parsing the Rust spelling; a field whose
  type is not a `ScriptField` stays as a comment naming that type; a component whose every field is like that,
  or whose field names the language cannot spell (a tuple index, or a reserved word —
  `UiInteraction.state`), is left undeclared with the reason. An empty `struct` means *marker*, and
  emitting one for a component that does carry data would be a false statement.
- **Systems (spec 10) are behaviors with no subject.** `system Name { … }` (`Item::System`, parsed by
  `parse_behavior`; no attributes) declares `Name` (`DeclaredKind::System`, Script domain) + `Name#state`;
  both registered `Runtime` + resumable, role `ScriptRole::System { module, order }`
  (`khora-io/src/script/declarations.rs`; a name changing kind on reload updates provenance/resumable).
  Checker (`types/query.rs::check_system`, `types/expr.rs`, `types/stmt.rs`): refuses `this`,
  `SUBJECT_NATIVES` (`Position`) directly and through any chain of free functions
  (`SubjectCalls`, refused at the system's call), `state`, `become`, `on`, `async`/`await`,
  `OnSpawn`/`OnDespawn`. A system's name is not in the checker's `components`: no script `Get`/`Set`/
  `Add`/`Remove`/`Has`/queries it. **One singleton entity per system**: the `script_systems` DataSystem
  (`khora-data/src/ecs/systems/script_systems.rs`, PreExtract after `script_components`, only while
  `runs_gameplay()`) finds or spawns `PersistentId::named("system:Name")` (`singleton_id`; `NAMED`
  namespace in `khora-core/src/ecs/persistent_id.rs` is fixed forever) holding `Name` at its defaults —
  the lane gives it `Name#state` like any behavior — and despawns it when the name is no longer a
  declared system. `flow/behaviors.rs::runs_on`: a system's row runs only on its singleton (other holders
  are data). `scene/capture.rs::scene_entities` drops every singleton from scenes **and** snapshots;
  `capture_game` keeps it (in `created`, under its named id). Only the declaring module's program runs a
  system it is imported into.
- **Queries (`Stmt::Query`).** `parser/stmt.rs::parse_foreach` → `parse_query` (a binding then `,`/`)`
  instead of `in`, or a leading `ref`); `with`/`without` are contextual idents, `with` first; `ref` and
  `system` are keywords. Checker `types/query.rs::check_query`: `Entity` once, never `?`/`ref`; each
  component once, never both named and excluded; at least one required (bound without `?`, or `with`);
  `ref` only in a system's members (free functions and behaviors are read-only), never on a behavior's
  component, never optional; plain bindings are read-only copies (`refuse_copy_write`), no binding is
  reassigned (`types/assign.rs`). Compile `bytecode/query.rs`: `QueryRows` (checkpoint, sized cost
  `1 + scan_cost`) snapshots `FrameComponents::matching(all, none)` — entity order, candidates = holders of
  the rarest required component, working set incl. this frame's adds/removes — into hidden local `#q`;
  per row `QueryHolds` re-check (skip), `GetComponent` per binding, plus a hidden original `#o<name>` per
  `ref`. Every named component joins `Program::reads`; queries fingerprinted by what they match.
- **`ref` write-back.** `Instruction::WriteRow` at the row's end (body end / `continue`; `end_row_for_break`,
  `end_rows_for_return`), cost `1 + fields`: fields differing from `#o` → `host.components.observe`
  (working set, visible at once) + `vm/rows.rs::PendingRows` keyed (component, field names; a row joins
  only the component's **latest** batch with exactly those fields). `send_rows` emits each batch as
  `WorldCommand::WriteColumn` before a later `WriteComponent`/`RemoveComponent` of that component in the
  run and at the end of **every** run (`Machine::run_counting`) — no batch outlives a run, none is frozen.
  `host.swept` → `ScriptRunReport.swept_rows`; the agent prices `2 µs × instances + PER_ROW (200 ns) × rows`.
- **Watchdog.** Constants in `khora-lanes/src/script_lane/watchdog.rs` (re-exported by `script_lane`),
  report entry `report.rs::Slow`. `Pending.running` = game seconds a pending body *ran* without finishing:
  the frame it starts and is cut (`turn.rs` `kept` + `park`) and every frame it is advanced and cut add `dt`;
  a frame its behavior sits out repaying a debt **that body** overdrew is counted as it passes
  (`watchdog::sit_out`, from the starvation path in `frame.rs`, debtor = `FuelShares::debtor`, only when the
  share has fuel — `own > 0`); `await` waits and frames not run for any other reason add nothing; an
  `await` resets it. Saved in the companion (`PendingSequence.running`, serde default 0), kept by every
  hot-reload tier; a load forgives the instance's debt (`FuelShares::forgive`). A due timer never starts,
  and a pending body is never resumed, with 0 fuel left. `WATCHDOG_WARN = 10 s`: one `log::warn!` + a
  `Slow` entry as the count crosses it. `WATCHDOG_FAULT = 60 s`: a fault like any (instance disabled,
  `#state.fault` fingerprint-stamped, re-enabled by an edit); the body is released (`turn.rs` `release`):
  a timer is rearmed, a hook hands its place to the body restored behind it. Every body kind, behaviors
  and systems.

## Resume (tiers)
One path for every resume — next frame, hot reload, game load (`vm/resume.rs::resume`):
1. **Exact** — same program fingerprint; 2. **Unchanged** — every function on the stack has its frozen
fingerprint; 3. **Rebuilt** — every frame's site still exists, locals matched by name + type, the same
number of temporaries; 4. **Restarted** — the member still takes the same parameters: rerun from entry
with the original arguments (side effects may repeat); 5. else **`Abandoned`** — the instance is owed
`OnResumeFailed(string member)` on its next turn (member like `"OnSpawn"`, `"Patrol.OnHit"`,
`"__every(0.5)"`).
- **Site grammar** (`bytecode/sites.rs`): `site := "entry" | path [":" point]`,
  `step := kind "." hash8 ["#" n]`, `branch := then|else|body|arm.hash8[#n]` (a `match` arm keyed by its pattern), `point := head | await[#n] | call.callee[#n]`.
  Named from what statements *say*, never from a counter, so an edit elsewhere renames nothing.
- **Keys are exhaustive** (`bytecode/keys.rs`): a compound statement is keyed by its header only. A new
  `Stmt`/`Expr` node must decide what it contributes, or two statements share a key.
- **The fingerprint normalizer is exhaustive** (`bytecode/fingerprint.rs`): an operand naming something
  outside the function (callee, native, literal, field) is hashed by name, never by index.
- **Timers** are functions named `Owner.__every(0.5)` / `Owner.__after(2)`, `#n` for identical siblings
  (omitted when zero) — the identity their countdown is saved under.
- **`FrozenMachine`** (engine terms, every encoding can read it): frames **name** their function, literals
  are kept as text, text built while running as `Text`; each `FrozenFrame` carries its `site`, function `fingerprint`, `locals` (name, type,
  scope, register) and `temporaries`; the machine carries the body's original `arguments`. Positions
  inside a function stay numbers, guarded by fingerprints.

## Saves and lifecycle
- A behavior's state companion `Guard#state` is `Runtime` + `resumable`: never in a scene, always in a
  game save — a guard saved mid-attack loads mid-attack.
- A system's component and its companion are `Runtime` + `resumable` on the singleton entity: never in a
  scene or a snapshot, always in a game save; a load finds `system:Name` again, never makes a second.
- `InstanceLifecycle { spawned, fault }` is **recorded**, not derived: an empty snapshot does not say
  whether `OnSpawn` ran. A fault recorded under another program fingerprint is cleared on load.
- `OnLoad` runs once, only when an instance is **restored from a save** — after its fields and
  restore, before anything else that turn. A scene load or editor Stop is a fresh start (`OnSpawn`).

## Traps
- `khora-script` depends on `khora-core` and `khora-macros` **only**, deliberately: the compiler and VM
  stay testable without booting an engine. Do not reach for `khora-data` from here.
- The rate that converts a time budget into fuel is measured by the **lane** and stored on
  `ScriptRuntime`, not assumed by the agent. `INITIAL_RATE` lives in
  `khora-lanes/src/script_lane/runtime.rs` and is the single definition.
- **A mirrored component a script cannot write stays an ordinary `struct`** (`Transform`, derived and
  runtime components): its fields read and write like any struct value's. A writable one is a `component`,
  read as a copy with `e.Get(T)`; `entity.Field` is a checker error naming `Get`
  (`types/calls.rs::entity_field`).
- `ENGINE_TYPES` (`khora-script/src/types/ty.rs`) is a list of names with a `Value` variant, a
  constructor and accessors behind them. It once named `Transform`, which had none, so `Transform t;`
  shaped cleanly and failed with the misleading "`Transform` has no `x`". Adding a name there without
  the three is worse than leaving it out — the mirrors give a component its fields through an ordinary
  `struct` declaration instead.

## Skills
- [`build-and-test`](../skills/build-and-test/SKILL.md) · [`debug-frame`](../skills/debug-frame/SKILL.md)
  · [`run-the-engine`](../skills/run-the-engine/SKILL.md).
- **Defaults are data.** A behavior field's and a state datum's default must be a constant (checker,
  `types/structs.rs::expect_behavior_constant`); a field with no default takes its type's neutral value
  (`Owned::neutral`), and a type with none (`Entity`) must be written optional. `BehaviorLayout.defaults` /
  `StateLayout.defaults` hold them; `BehaviorLayout::fresh_store()` builds a fresh instance (fields, first
  state entered, timers armed) with no VM run and no fuel; `entry(state)` is what entering a state writes,
  and `Instruction::Become` writes it from the layout at run time — so editing only a default leaves the
  program fingerprint alone. There is no initialiser: no `__fields`/`__enter`, no `initialised`/`carried`.
- **Hot reload in step.** The pump announces each recompiled program's reads
  (`ScriptCatalog::announce`); the flow projects `reads ∪ announced`; the lane installs before any turn and
  `settle`s. A module whose reload still waits takes no turn (deferred, events kept). An instance's
  `built_for` program differing from the one about to run — a behavior moved to another module — is carried
  at the turn (`script_lane/carry.rs::carry_instance`, shared with `ScriptRuntime::reload`); a carried
  instance is `rewritten` and emits its `BehaviorUpdate` that frame even starved or quiet. An instance whose
  arrival is held is neither carried nor recorded: the arrival rebuilds it.
- **One write order.** A behavior is a component other scripts `Set`/`Get`/`Has` (`BehaviorDecl::as_component`);
  `this.Set(<own behavior>)` is refused. A turn starts from the `Set`s of its fields emitted earlier in the
  frame (`state_update::sets_on`); `BehaviorUpdate.read_until` / `emitted_after` = frame commands queued when it
  began / ended (rebased onto the deck slot); `changed` is against the row with those earlier `Set`s. `script_commands` records `AppliedWrites` (last applied index per field /
  whole component); `behavior_writeback` drops a turn field written at or after `emitted_after`, drops all of
  them when the component was written whole in the frame, and `behavior_writeback/order.rs` reports each
  behavior field written more than once (debug) — the turn plus the `Set`s it did not read — instead of
  `script_commands`.
