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

//! A sweep cut part-way and taken back by edited code.
//!
//! A hot reload that edits code outside a query rebuilds a frame stopped
//! anywhere in its sweep — a nested one included — at the same sites: the
//! rows it took, the row it stood on and its bindings come back, and the
//! sweep finishes visiting each row once. A nested sweep handed fuel slice by
//! slice does the same within the overdraft bound. A function's fingerprint
//! names its queries by the components they match, never by where the
//! program lists them.

use std::collections::HashMap;
use std::sync::Arc;

use crate::support::fresh_store;
use khora_core::ecs::entity::EntityId;
use khora_core::script::{
    ColumnSnapshot, ComponentName, ComponentProjection, FrameComponents, PendingBody,
    ProjectedComponent, ScriptValue,
};
use khora_script::arena::Persisted;
use khora_script::vm::{resume, Machine, Program, ResumeTier, Run, Suspension, Value};
use khora_script::{compile_module, Host, MemoryLoader, ENGINE_COMPONENTS_MODULE};

const SUBJECT: EntityId = EntityId {
    index: 1,
    generation: 1,
};

const ENGINE: &str = "
component Health { int current; int max; }
component Regen { float rate; }
component Dead { float since; }
";

fn build(main: &str) -> Program {
    let loader = MemoryLoader::new()
        .with(ENGINE_COMPONENTS_MODULE, ENGINE)
        .with(
            "main.erg",
            format!("import \"{ENGINE_COMPONENTS_MODULE}\";\n{main}"),
        );
    let result = compile_module(&loader, "main.erg");
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "should compile:\n{main}\n{errors:?}");
    result.program.expect("a program with no errors")
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

fn struct_of(pairs: &[(&str, ScriptValue)]) -> ScriptValue {
    ScriptValue::Struct(
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect(),
    )
}

fn projected(
    name: &str,
    holders: &[(u32, ScriptValue)],
    default: ScriptValue,
) -> ProjectedComponent {
    let rows = holders.iter().map(|(_, row)| row.clone()).collect();
    let index: HashMap<EntityId, (u32, u32)> = holders
        .iter()
        .enumerate()
        .map(|(row, (index, _))| {
            (
                EntityId {
                    index: *index,
                    generation: 1,
                },
                (0, row as u32),
            )
        })
        .collect();
    ProjectedComponent::new(
        ComponentName::new(name),
        vec![Box::new(Rows(rows))],
        index,
        Some(default),
    )
}

/// Entities 10, 20, … 70 hold `Health` with `current` 1 … 7 and `Regen`;
/// entity 40 is `Dead`.
fn projection() -> ComponentProjection {
    let health = |current: i64| {
        struct_of(&[
            ("current", ScriptValue::Int(current)),
            ("max", ScriptValue::Int(10)),
        ])
    };
    let regen = struct_of(&[("rate", ScriptValue::Float(1.0))]);
    let dead = struct_of(&[("since", ScriptValue::Float(0.0))]);
    let holders: Vec<(u32, ScriptValue)> = [7, 3, 1, 6, 4, 2, 5]
        .into_iter()
        .map(|n| (n * 10, health(i64::from(n))))
        .collect();
    let regens: Vec<(u32, ScriptValue)> = [5, 1, 7, 2, 6, 3, 4]
        .into_iter()
        .map(|n| (n * 10, regen.clone()))
        .collect();
    ComponentProjection::new(vec![
        projected("Health", &holders, health(100)),
        projected("Regen", &regens, regen.clone()),
        projected("Dead", &[(40, dead.clone())], dead),
    ])
}

/// The rows of `without Dead`, folded `total * 10 + current`.
const VISITED: i64 = 123_567;

fn host_over(fields: khora_script::arena::PersistentStore) -> Host {
    let mut host = Host::new().with_fields(fields).for_entity(SUBJECT);
    host.components = FrameComponents::new(Arc::new(projection()));
    host
}

fn total(program: &Program, host: &Host) -> Option<i64> {
    let slot = program.layout("Census")?.slot_of("total")?;
    match host.fields.get(slot) {
        Some(Persisted::Scalar(Value::Int(value))) => Some(*value),
        _ => None,
    }
}

/// `Census.Update` with `sweep` as its query, and `after` once it is done.
fn census(sweep: &str, after: &str) -> Program {
    build(&format!(
        "behavior Census {{
             int total = 0;
             void Update(float dt) {{
                 total = 0;
                 {sweep}
                 {after}
             }}
         }}"
    ))
}

const FLAT: &str = "foreach (Entity e, Health h, Regen r) without Dead {
    int seen = h.current;
    total = total * 10 + seen;
}";

/// Each outer row finds itself once among the inner rows.
const NESTED: &str = "foreach (Entity e, Health h) without Dead {
    int seen = h.current;
    foreach (Health k, Regen r) {
        if (k.current == seen) { total = total * 10 + seen; }
    }
}";

/// **A sweep cut anywhere is rebuilt into an edit after it.** Stopped for
/// fuel at every point of `Update`, frozen, and resumed into the same body
/// with a statement added after the query, the frame is rebuilt — never
/// restarted — and finishes the sweep it was in: every row once, in order.
#[test]
fn a_sweep_frozen_part_way_is_rebuilt_into_an_edited_body() {
    for (label, sweep) in [("flat", FLAT), ("nested", NESTED)] {
        let original = census(sweep, "");
        let edited = census(sweep, "total = total + 0;");

        let mut stops = 0;
        for fuel in 1..=400 {
            let mut host = host_over(fresh_store(&original, "Census"));
            let mut machine =
                Machine::new(&original, "Census.Update", &[Value::Float(0.1)]).expect("Update");
            match machine.run(&original, &mut host, fuel) {
                Run::Completed => break,
                Run::Suspended(Suspension::OutOfFuel) => {}
                other => panic!("{label}, fuel {fuel}: {other:?}"),
            }
            stops += 1;
            host.end_frame();
            let frozen = machine
                .freeze(&original, PendingBody::Update)
                .unwrap_or_else(|| panic!("{label}: a sweep stopped at fuel {fuel} freezes"));

            let (mut thawed, tier) = resume(&frozen, original.fingerprint(), &edited)
                .unwrap_or_else(|abandoned| panic!("{label}, fuel {fuel}: {abandoned:?}"));
            assert_eq!(
                tier,
                ResumeTier::Rebuilt,
                "{label}: a frame stopped at fuel {fuel} is rebuilt at its site"
            );
            let mut next = host_over(host.fields.clone());
            assert_eq!(
                thawed.run(&edited, &mut next, u64::MAX),
                Run::Completed,
                "{label}, fuel {fuel}"
            );
            assert_eq!(
                total(&edited, &next),
                Some(VISITED),
                "{label}: every row once, in order, rebuilt after fuel {fuel}"
            );
        }
        assert!(stops > 20, "{label}: the sweep stopped only {stops} times");
    }
}

/// **A nested sweep short of fuel resumes where it stopped**, slice after
/// slice, every run within the program's overdraft bound.
#[test]
fn a_nested_sweep_short_of_fuel_visits_each_pair_once() {
    let program = census(NESTED, "");
    for slice in 1..=20 {
        let mut host = host_over(fresh_store(&program, "Census"));
        let mut machine =
            Machine::new(&program, "Census.Update", &[Value::Float(0.1)]).expect("Update");
        let mut runs = 0;
        loop {
            runs += 1;
            assert!(runs < 100_000, "no end at slice {slice}");
            let (outcome, spent) = machine.run_counting(&program, &mut host, slice);
            assert!(
                spent <= slice + program.max_overdraft(),
                "slice {slice}: spent {spent}, bound {}",
                program.max_overdraft()
            );
            match outcome {
                Run::Completed => break,
                Run::Suspended(Suspension::OutOfFuel) => {
                    host.end_frame();
                }
                other => panic!("slice {slice}: {other:?}"),
            }
        }
        assert_eq!(
            total(&program, &host),
            Some(VISITED),
            "every outer row once, at slice {slice}"
        );
    }
}

/// **A query is fingerprinted by what it matches.** Another function's query
/// listed before it leaves a function's fingerprint alone; a different
/// `without`, `with` or bound component changes it.
#[test]
fn a_function_fingerprint_names_its_queries_not_their_index() {
    let update = |program: &Program| {
        program
            .function("Census.Update")
            .expect("`Update` is a function")
            .fingerprint
    };
    let behavior = |header: &str| {
        format!(
            "behavior Census {{
                 int total = 0;
                 void Update(float dt) {{ {header} {{ total += 1; }} }}
             }}"
        )
    };
    let base = build(&behavior("foreach (Health h) without Dead"));

    let shifted = build(&format!(
        "fn int Count() {{ int n = 0; foreach (Regen r) {{ n += 1; }} return n; }}\n{}",
        behavior("foreach (Health h) without Dead")
    ));
    assert_eq!(
        update(&base),
        update(&shifted),
        "a query listed before this one changes nothing here"
    );

    for header in [
        "foreach (Health h) without Regen",
        "foreach (Health h) with Dead",
        "foreach (Regen h) without Dead",
        "foreach (Health h) with Regen without Dead",
    ] {
        assert_ne!(
            update(&base),
            update(&build(&behavior(header))),
            "`{header}` matches other rows"
        );
    }
}
