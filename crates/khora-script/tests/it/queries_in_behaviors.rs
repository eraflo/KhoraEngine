// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! A behavior sweeps entities with a query, read-only:
//! `foreach (Entity e, Health h, Regen r) with Player without Dead { … }`.
//!
//! What the language promises before anything reaches a `World`: which
//! components a query adds to the program's read set, which rows it visits
//! over a working set built by hand — every bound non-optional component and
//! every `with` held, no `without` held, in entity order, this frame's adds,
//! removes and writes included — what each binding holds, and what the
//! checker refuses: a `ref` binding (a behavior writes other entities through
//! `Set`), a write to a binding, a binding that is not a component.

use std::collections::HashMap;
use std::sync::Arc;

use crate::support::fresh_store;
use khora_core::ecs::entity::EntityId;
use khora_core::script::{
    ColumnSnapshot, ComponentName, ComponentProjection, FrameComponents, ProjectedComponent,
    ScriptValue,
};
use khora_script::arena::Persisted;
use khora_script::vm::{Machine, Program, Run, Value};
use khora_script::{
    compile_module, CompileOutcome, Diagnostic, Host, MemoryLoader, ENGINE_COMPONENTS_MODULE,
};

/// The behavior running the query; it holds none of the queried components.
const SUBJECT: EntityId = EntityId {
    index: 1,
    generation: 1,
};

/// Entity `index`, generation 1.
const fn entity(index: u32) -> EntityId {
    EntityId {
        index,
        generation: 1,
    }
}

// Six entities, in entity order. Each one's `Health.current` is its ordinal,
// so a sweep folding `total * 10 + h.current` spells the rows it visited, in
// the order it visited them, each as many times as it did.
//
// | entity | Health | Regen | Player | Dead | Shield |
// |--------|--------|-------|--------|------|--------|
// | E1     | 1      | 0.5   | yes    |      |        |
// | E2     | 2      | 1.0   | yes    | yes  |        |
// | E3     | 3      | 1.5   |        |      |        |
// | E4     | 4      |       | yes    |      |        |
// | E5     | 5      | 2.5   | yes    |      | 7      |
// | E6     | 6      | 3.0   | yes    |      |        |
const E1: EntityId = entity(3);
const E2: EntityId = entity(7);
const E3: EntityId = entity(12);
const E4: EntityId = entity(20);
const E5: EntityId = entity(31);
const E6: EntityId = entity(45);

/// The engine module, as the mirror would serve it.
const ENGINE: &str = "
component Health { int current; int max; }
component Regen { float rate; }
component Player { int team; }
component Dead { float since; }
component Shield { int amount; }
component Ghost { int level; }
";

fn compile(main: &str) -> CompileOutcome {
    let loader = MemoryLoader::new()
        .with(ENGINE_COMPONENTS_MODULE, ENGINE)
        .with(
            "main.erg",
            format!("import \"{ENGINE_COMPONENTS_MODULE}\";\n{main}"),
        );
    compile_module(&loader, "main.erg")
}

fn said(diagnostic: &Diagnostic) -> String {
    format!(
        "{} | {}",
        diagnostic.message,
        diagnostic.note.as_deref().unwrap_or_default()
    )
}

fn errors_of(result: &CompileOutcome) -> Vec<String> {
    result
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(said)
        .collect()
}

fn build(main: &str) -> Program {
    let result = compile(main);
    let errors = errors_of(&result);
    assert!(errors.is_empty(), "should compile:\n{main}\n{errors:?}");
    result.program.expect("a program with no errors")
}

/// The refusal of `main` saying every one of `fragments` and underlining
/// `snippet` — after proving `control`, the same program written right,
/// compiles, so the refusal is about what differs.
fn refused(control: &str, main: &str, fragments: &[&str], snippet: &str) -> Diagnostic {
    build(control);
    let result = compile(main);
    assert!(
        !result.succeeded(),
        "should be refused, and compiled:\n{main}"
    );
    let source = format!("import \"{ENGINE_COMPONENTS_MODULE}\";\n{main}");
    result
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .find(|d| {
            let text = said(d);
            let start = d.span.start as usize;
            let end = d.span.end as usize;
            fragments.iter().all(|fragment| text.contains(fragment))
                && source
                    .get(start..end)
                    .is_some_and(|underlined| underlined.contains(snippet))
        })
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "expected a refusal saying {fragments:?} at `{snippet}`; got {:?}",
                result
                    .diagnostics
                    .iter()
                    .map(|d| {
                        let underlined = source
                            .get(d.span.start as usize..d.span.end as usize)
                            .unwrap_or("<out of the file>");
                        format!("{} @ {underlined:?}", said(d))
                    })
                    .collect::<Vec<_>>()
            )
        })
}

fn fields(pairs: &[(&str, ScriptValue)]) -> ScriptValue {
    ScriptValue::Struct(
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect(),
    )
}

/// A column of rows already converted.
struct Rows(Vec<ScriptValue>);

impl ColumnSnapshot for Rows {
    fn len(&self) -> usize {
        self.0.len()
    }
    fn read(&self, row: usize) -> ScriptValue {
        self.0[row].clone()
    }
}

/// Component `name` held by `holders`, one column in the order given — never
/// entity order, so a sweep in entity order is the sweep's own doing.
fn projected(
    name: &str,
    holders: &[(EntityId, ScriptValue)],
    default: ScriptValue,
) -> ProjectedComponent {
    let rows = holders.iter().map(|(_, row)| row.clone()).collect();
    let index: HashMap<EntityId, (u32, u32)> = holders
        .iter()
        .enumerate()
        .map(|(row, (entity, _))| (*entity, (0, row as u32)))
        .collect();
    ProjectedComponent::new(
        ComponentName::new(name),
        vec![Box::new(Rows(rows))],
        index,
        Some(default),
    )
}

fn health(current: i64) -> ScriptValue {
    fields(&[
        ("current", ScriptValue::Int(current)),
        ("max", ScriptValue::Int(10)),
    ])
}

fn regen(rate: f32) -> ScriptValue {
    fields(&[("rate", ScriptValue::Float(rate))])
}

fn player() -> ScriptValue {
    fields(&[("team", ScriptValue::Int(1))])
}

/// What the flow would project — the table above, plus a behavior `Guard`
/// run by E1 and E5.
fn projection() -> ComponentProjection {
    ComponentProjection::new(vec![
        projected(
            "Health",
            &[
                (E6, health(6)),
                (E3, health(3)),
                (E1, health(1)),
                (E5, health(5)),
                (E2, health(2)),
                (E4, health(4)),
            ],
            health(100),
        ),
        projected(
            "Regen",
            &[
                (E5, regen(2.5)),
                (E1, regen(0.5)),
                (E6, regen(3.0)),
                (E2, regen(1.0)),
                (E3, regen(1.5)),
            ],
            regen(0.0),
        ),
        projected(
            "Player",
            &[
                (E4, player()),
                (E1, player()),
                (E6, player()),
                (E5, player()),
                (E2, player()),
            ],
            player(),
        ),
        projected(
            "Dead",
            &[(E2, fields(&[("since", ScriptValue::Float(1.0))]))],
            fields(&[("since", ScriptValue::Float(0.0))]),
        ),
        projected(
            "Shield",
            &[(E5, fields(&[("amount", ScriptValue::Int(7))]))],
            fields(&[("amount", ScriptValue::Int(0))]),
        ),
        projected("Ghost", &[], fields(&[("level", ScriptValue::Int(0))])),
        projected(
            "Guard",
            &[
                (E5, fields(&[("health", ScriptValue::Float(4.0))])),
                (E1, fields(&[("health", ScriptValue::Float(2.5))])),
            ],
            fields(&[("health", ScriptValue::Float(10.0))]),
        ),
    ])
}

/// A host for `behavior`'s instance on the subject, over the projection.
fn host_for(program: &Program, behavior: &str) -> Host {
    let mut host = Host::new()
        .with_fields(fresh_store(program, behavior))
        .for_entity(SUBJECT);
    host.components = FrameComponents::new(Arc::new(projection()));
    host
}

/// Runs `Behavior.Method` whole on a fresh instance, returning its result.
fn run(program: &Program, function: &str, args: &[Value]) -> Value {
    let behavior = function
        .split_once('.')
        .map(|(behavior, _)| behavior)
        .expect("a behavior's method");
    let mut host = host_for(program, behavior);
    let mut machine = Machine::new(program, function, args).expect("the function exists");
    let outcome = machine.run(program, &mut host, u64::MAX);
    assert_eq!(outcome, Run::Completed, "`{function}` completes");
    machine.result()
}

/// `Census` with `members`.
fn census(members: &str) -> Program {
    build(&format!("behavior Census {{ {members} }}"))
}

/// A `Census.Total` folding `total * 10 + h.current` over `query` (a header,
/// binding `h`), and its result.
fn visited(query: &str) -> Value {
    let program = census(&format!(
        "int Total() {{
             int total = 0;
             {query} {{ total = total * 10 + h.current; }}
             return total;
         }}"
    ));
    run(&program, "Census.Total", &[])
}

// ─── What a query reads ─────────────────────────────────────────────────────

/// **Every component a query names is read**: its bindings, optional or not,
/// its `with` and its `without` — so the flow projects them all.
#[test]
fn a_query_lists_every_component_it_names_in_the_reads() {
    let program = census(
        "int total = 0;
         void Update(float dt) {
             foreach (Entity e, Health h, Regen r, Shield? s) with Player without Dead {
                 total += h.current;
             }
         }",
    );

    for component in ["Health", "Regen", "Shield", "Player", "Dead"] {
        assert!(
            program.reads.iter().any(|read| read == component),
            "`{component}` is read: {:?}",
            program.reads
        );
    }
    assert!(
        !program.reads.iter().any(|read| read == "Ghost"),
        "a component no query names is not: {:?}",
        program.reads
    );
}

// ─── Which rows a query visits ──────────────────────────────────────────────

/// **Exactly the matching entities, in entity order, each once**: every bound
/// component and every `with` held, no `without` held — whatever order the
/// projection lists its rows in.
#[test]
fn a_query_visits_exactly_the_matching_entities_in_entity_order() {
    assert_eq!(
        visited("foreach (Health h, Regen r)"),
        Value::Int(12356),
        "every holder of both"
    );
    assert_eq!(
        visited("foreach (Health h, Regen r) with Player"),
        Value::Int(1256),
        "`with` keeps the holders of `Player`"
    );
    assert_eq!(
        visited("foreach (Health h, Regen r) without Dead"),
        Value::Int(1356),
        "`without` drops the holders of `Dead`"
    );
    assert_eq!(
        visited("foreach (Entity e, Health h, Regen r) with Player without Dead"),
        Value::Int(156),
        "both"
    );
    assert_eq!(
        visited("foreach (Health h)"),
        Value::Int(123456),
        "one binding: every holder"
    );
}

/// The query of the plan, folding into a behavior field: `total += h.current`
/// over E1, E5 and E6.
#[test]
fn a_query_accumulates_into_a_behavior_field() {
    let program = census(
        "int total = 0;
         void Update(float dt) {
             foreach (Entity e, Health h, Regen r) with Player without Dead {
                 total += h.current;
             }
         }",
    );
    let mut host = host_for(&program, "Census");
    let mut machine =
        Machine::new(&program, "Census.Update", &[Value::Float(0.5)]).expect("`Update`");
    assert_eq!(machine.run(&program, &mut host, u64::MAX), Run::Completed);

    let layout = program.layout("Census").expect("a layout");
    let slot = layout.slot_of("total").expect("a `total` field");
    assert_eq!(
        host.fields.get(slot),
        Some(&Persisted::Scalar(Value::Int(12))),
        "1 + 5 + 6"
    );
}

/// **Each binding holds its row's copy**: the fields of the entity the row
/// is, read like a struct.
#[test]
fn each_binding_holds_its_rows_values() {
    let program = census(
        "float Rates() {
             float sum = 0.0;
             foreach (Regen r, Health h) with Player without Dead { sum += r.rate; }
             return sum;
         }
         int Maxima() {
             int sum = 0;
             foreach (Regen r, Health h) with Player without Dead { sum += h.max; }
             return sum;
         }",
    );

    assert_eq!(
        run(&program, "Census.Rates", &[]),
        Value::Float(6.0),
        "0.5 + 2.5 + 3.0"
    );
    assert_eq!(run(&program, "Census.Maxima", &[]), Value::Int(30));
}

/// **`T?` binds `null` where the entity lacks `T`**, and never filters: the
/// rows are those of the other bindings.
#[test]
fn an_optional_binding_is_null_where_the_entity_lacks_the_component() {
    let program = census(
        "int Shields() {
             int total = 0;
             foreach (Health h, Shield? s) with Player without Dead {
                 total = total * 100 + h.current * 10 + (s?.amount ?? 0);
             }
             return total;
         }
         int Absent() {
             int none = 0;
             foreach (Health h, Shield? s) with Player without Dead {
                 if (s == null) { none += 1; }
             }
             return none;
         }",
    );

    // E1 → 10, E4 → 40, E5 → 57, E6 → 60.
    assert_eq!(
        run(&program, "Census.Shields", &[]),
        Value::Int(10_40_57_60)
    );
    assert_eq!(run(&program, "Census.Absent", &[]), Value::Int(3));
}

/// **`Entity e` is the row's entity, wherever it is written** — first, between
/// the components, or last.
#[test]
fn the_entity_binding_names_the_row_entity_at_any_position() {
    for header in [
        "foreach (Entity e, Health h, Regen r) with Player without Dead",
        "foreach (Health h, Entity e, Regen r) with Player without Dead",
        "foreach (Health h, Regen r, Entity e) with Player without Dead",
    ] {
        let program = census(&format!(
            "int Total() {{
                 int total = 0;
                 {header} {{ total = total * 10 + (e.Get(Health)?.current ?? 0); }}
                 return total;
             }}"
        ));
        assert_eq!(
            run(&program, "Census.Total", &[]),
            Value::Int(156),
            "{header}"
        );
    }
}

/// **A query inside a query** sweeps its rows once per outer row.
#[test]
fn a_nested_query_runs_its_inner_sweep_for_every_outer_row() {
    let program = census(
        "int Pairs() {
             int pairs = 0;
             int same = 0;
             foreach (Entity a, Player p) without Dead {
                 foreach (Entity b, Regen r) with Player without Dead {
                     pairs += 1;
                     if ((a.Get(Health)?.current ?? 0) == (b.Get(Health)?.current ?? -1)) {
                         same += 1;
                     }
                 }
             }
             return pairs * 10 + same;
         }",
    );

    // Outer: E1, E4, E5, E6. Inner: E1, E5, E6. 4 × 3 pairs, 3 of them alike.
    assert_eq!(run(&program, "Census.Pairs", &[]), Value::Int(123));
}

/// **No match, no turn**: a query whose rows are empty runs its body zero
/// times — a component nobody holds, a `with` nobody holds, a `without` every
/// candidate holds.
#[test]
fn a_query_over_no_matching_entity_runs_its_body_zero_times() {
    for query in [
        "foreach (Health h, Ghost g)",
        "foreach (Health h) with Ghost",
        "foreach (Regen r, Player p) without Health",
    ] {
        let program = census(&format!(
            "int Turns() {{
                 int turns = 0;
                 {query} {{ turns += 1; }}
                 return turns;
             }}"
        ));
        assert_eq!(run(&program, "Census.Turns", &[]), Value::Int(0), "{query}");
    }
}

// ─── The working set ────────────────────────────────────────────────────────

/// **A query sees this frame's structural changes**: a component added
/// earlier in the frame makes its entity a row, one removed takes it out — of
/// a binding, of a `with`, through a `without`.
#[test]
fn a_query_sees_components_added_and_removed_earlier_in_the_frame() {
    let program = census(
        "int Sweep() {
             int total = 0;
             foreach (Health h, Regen r) with Player without Dead { total = total * 10 + h.current; }
             return total;
         }
         int RegenAdded(Entity target) { target.Add(Regen { rate: 9.0 }); return Sweep(); }
         int RegenRemoved(Entity target) { target.Remove(Regen); return Sweep(); }
         int PlayerAdded(Entity target) { target.Add(Player { team: 2 }); return Sweep(); }
         int DeadAdded(Entity target) { target.Add(Dead { since: 0.0 }); return Sweep(); }
         int DeadRemoved(Entity target) { target.Remove(Dead); return Sweep(); }",
    );

    for (function, target, expected) in [
        ("Census.RegenAdded", E4, 1456),
        ("Census.RegenRemoved", E5, 16),
        ("Census.PlayerAdded", E3, 1356),
        ("Census.DeadAdded", E1, 56),
        ("Census.DeadRemoved", E2, 1256),
    ] {
        assert_eq!(
            run(&program, function, &[Value::Entity(target)]),
            Value::Int(expected),
            "`{function}`"
        );
    }
}

/// **A row reads the working set's copy**: a value written earlier in the
/// frame, not the projected one.
#[test]
fn a_query_reads_values_written_earlier_in_the_frame() {
    let program = census(
        "int Healed(Entity target) {
             target.Set(Health { current: 8 });
             int total = 0;
             foreach (Health h, Regen r) with Player without Dead { total = total * 10 + h.current; }
             return total;
         }",
    );

    assert_eq!(
        run(&program, "Census.Healed", &[Value::Entity(E1)]),
        Value::Int(856)
    );
}

/// **The rows are taken when the sweep starts.** A structural change made in
/// the body — the row's own entity losing a bound component, gaining a
/// `without` — is a command: every row is still visited once, and a sweep
/// started after it sees the change.
#[test]
fn a_structural_change_in_the_body_does_not_disturb_the_sweep() {
    let program = census(
        "int Sweep() {
             int total = 0;
             foreach (Entity e, Health h, Regen r) with Player without Dead {
                 e.Remove(Regen);
                 e.Add(Dead { since: 0.0 });
                 total = total * 10 + h.current;
             }
             int later = 0;
             foreach (Health h, Regen r) with Player without Dead { later += 1; }
             return total * 10 + later;
         }",
    );

    assert_eq!(run(&program, "Census.Sweep", &[]), Value::Int(1560));
}

/// **A behavior's component is queried like any other**, read-only.
#[test]
fn a_query_reads_a_behavior_component() {
    let program = build(
        "behavior Guard { float health = 10; }
         behavior Census {
             float Guarded() {
                 float sum = 0.0;
                 foreach (Entity e, Guard g) { sum += g.health; }
                 return sum;
             }
         }",
    );

    assert!(
        program.reads.iter().any(|read| read == "Guard"),
        "`Guard` is read: {:?}",
        program.reads
    );
    assert_eq!(run(&program, "Census.Guarded", &[]), Value::Float(6.5));
}

// ─── What the checker refuses ───────────────────────────────────────────────

/// A query a behavior may write: the control of every refusal below.
const WELL_FORMED: &str = "behavior Census {
    int total = 0;
    void Update(float dt) {
        foreach (Entity e, Health h, Regen r) with Player without Dead {
            total += h.current;
        }
    }
}";

/// **A behavior's query is read-only.** `ref` is refused at the binding, and
/// the note points at `Set`, which is how a behavior writes another entity.
#[test]
fn a_ref_binding_in_a_behavior_is_refused_pointing_to_set() {
    let refusal = refused(
        WELL_FORMED,
        "behavior Census {
             void Update(float dt) {
                 foreach (Entity e, ref Health h, Regen r) { }
             }
         }",
        &["read-only"],
        "ref Health",
    );
    assert!(
        refusal.note.as_deref().unwrap_or_default().contains("Set"),
        "the note points at `Set`: {}",
        said(&refusal)
    );
}

/// **A plain binding is read-only**: writing one of its fields — directly,
/// compound, nested — is refused at the write, as a copy's is.
#[test]
fn a_plain_binding_is_read_only() {
    for (write, snippet) in [
        ("h.current = 1;", "h.current"),
        ("h.current += 1;", "h.current"),
        ("r.rate = 2.0;", "r.rate"),
    ] {
        let main = format!(
            "behavior Census {{
                 void Update(float dt) {{
                     foreach (Health h, Regen r) {{ {write} }}
                 }}
             }}"
        );
        let name = &snippet[..1];
        refused(
            WELL_FORMED,
            &main,
            &[&format!("`{name}`"), "read-only"],
            snippet,
        );
    }
}

/// **A binding is not a variable**: assigning the binding itself is refused.
#[test]
fn a_query_binding_cannot_be_reassigned() {
    refused(
        WELL_FORMED,
        "behavior Census {
             void Update(float dt) {
                 foreach (Health h) {
                     foreach (Health k) { h = k; }
                 }
             }
         }",
        &["`h`"],
        "h = k",
    );
    refused(
        WELL_FORMED,
        "behavior Census {
             void Update(float dt) {
                 foreach (Entity e, Health h) {
                     foreach (Entity f, Health k) { e = f; }
                 }
             }
         }",
        &["`e`"],
        "e = f",
    );
}

/// **A binding names a component**: a struct, a scalar or a name nobody
/// declared is refused at the binding.
#[test]
fn a_query_binding_must_name_a_component() {
    for (binding, name) in [
        ("Loot l", "Loot"),
        ("int n", "int"),
        ("Missing m", "Missing"),
    ] {
        refused(
            WELL_FORMED,
            &format!(
                "struct Loot {{ int value = 0; }}
                 behavior Census {{
                     void Update(float dt) {{ foreach (Health h, {binding}) {{ }} }}
                 }}"
            ),
            &[&format!("`{name}`")],
            binding,
        );
    }
}

/// **A behavior's state companion is the engine's**: no query names it,
/// though the behavior's own component may be queried.
#[test]
fn a_query_binding_cannot_name_a_state_companion() {
    let control = "behavior Guard { float health = 10; }
         behavior Census { void Update(float dt) { foreach (Guard g) { } } }";
    build(control);

    let result = compile(
        "behavior Guard { float health = 10; }
         behavior Census { void Update(float dt) { foreach (Guard#state g) { } } }",
    );
    assert!(
        !result.succeeded(),
        "a query cannot name `Guard#state`: {:?}",
        errors_of(&result)
    );
}

/// **A component is bound once per query**: the second binding of it is
/// refused at itself.
#[test]
fn a_query_binding_cannot_repeat_a_component() {
    let main = "behavior Census {
             void Update(float dt) {
                 foreach (Health a, Regen r, Health b) { }
             }
         }";
    refused(WELL_FORMED, main, &["`Health`"], "Health b");
    refused(
        WELL_FORMED,
        "behavior Census {
             void Update(float dt) {
                 foreach (Health a, Health? b) { }
             }
         }",
        &["`Health`"],
        "Health? b",
    );
}

/// **`with` and `without` name components**: a struct or an undeclared name
/// is refused at itself.
#[test]
fn with_and_without_must_name_components() {
    for (filters, name) in [
        ("with Loot", "Loot"),
        ("without Loot", "Loot"),
        ("with Player without Missing", "Missing"),
    ] {
        refused(
            WELL_FORMED,
            &format!(
                "struct Loot {{ int value = 0; }}
                 behavior Census {{
                     void Update(float dt) {{ foreach (Health h) {filters} {{ }} }}
                 }}"
            ),
            &[&format!("`{name}`")],
            name,
        );
    }
}
