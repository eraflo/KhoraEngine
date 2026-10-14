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

//! A sweep short of fuel stops where it is and resumes there.
//!
//! A query's body is a body like any other: out of fuel, it stops at its next
//! safepoint — mid-row or between rows — and the next run carries on from
//! that very point, so no row is skipped and none is visited twice. A machine
//! frozen there (a save, a hot reload) is thawed onto the same row.

use std::collections::HashMap;
use std::sync::Arc;

use crate::support::fresh_store;
use khora_core::ecs::entity::EntityId;
use khora_core::script::{
    ColumnSnapshot, ComponentName, ComponentProjection, FrameComponents, PendingBody,
    ProjectedComponent, ScriptValue,
};
use khora_script::arena::Persisted;
use khora_script::vm::{resume, Machine, Program, Run, Suspension, Value};
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

/// `Census`: `Update` folds `total * 10 + h.current` over its sweep, so
/// `total` spells the rows visited, in order, each as often as it was.
const CENSUS: &str = "behavior Census {
    int total = 0;
    void Update(float dt) {
        total = 0;
        foreach (Entity e, Health h, Regen r) without Dead {
            int seen = h.current;
            total = total * 10 + seen;
        }
    }
}";

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

fn struct_of(pairs: &[(&str, ScriptValue)]) -> ScriptValue {
    ScriptValue::Struct(
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect(),
    )
}

/// Entities 10, 20, … 70 hold `Health` with `current` 1 … 7 and `Regen`;
/// entity 40 is `Dead`. The sweep visits 1, 2, 3, 5, 6, 7.
fn projection() -> ComponentProjection {
    let health = |current: i64| {
        struct_of(&[
            ("current", ScriptValue::Int(current)),
            ("max", ScriptValue::Int(10)),
        ])
    };
    let regen = struct_of(&[("rate", ScriptValue::Float(1.0))]);
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
        projected(
            "Dead",
            &[(40, struct_of(&[("since", ScriptValue::Float(0.0))]))],
            struct_of(&[("since", ScriptValue::Float(0.0))]),
        ),
    ])
}

const VISITED: i64 = 123_567;

fn host_for(program: &Program) -> Host {
    let mut host = Host::new()
        .with_fields(fresh_store(program, "Census"))
        .for_entity(SUBJECT);
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

/// **A sweep resumed run after run visits each row once.** Handed `slice`
/// fuel per run, a frame ending between runs, `Update` stops many times — in
/// the middle of rows and between them — and ends with every row visited
/// once, in entity order: the same total as a whole run.
#[test]
fn a_query_short_of_fuel_resumes_at_the_row_it_stopped_on() {
    let program = build(CENSUS);

    let mut whole = host_for(&program);
    let mut machine =
        Machine::new(&program, "Census.Update", &[Value::Float(0.1)]).expect("Update");
    assert_eq!(machine.run(&program, &mut whole, u64::MAX), Run::Completed);
    assert_eq!(total(&program, &whole), Some(VISITED), "the whole run");

    for slice in 1..=12 {
        let mut host = host_for(&program);
        let mut machine =
            Machine::new(&program, "Census.Update", &[Value::Float(0.1)]).expect("Update");
        let mut runs = 0;
        loop {
            runs += 1;
            assert!(runs < 10_000, "no end at slice {slice}");
            match machine.run(&program, &mut host, slice) {
                Run::Completed => break,
                Run::Suspended(Suspension::OutOfFuel) => {
                    host.end_frame();
                }
                other => panic!("slice {slice}: {other:?}"),
            }
        }
        assert!(runs > 6, "slice {slice} stopped only {runs} times");
        assert_eq!(
            total(&program, &host),
            Some(VISITED),
            "every row once, in order, at slice {slice}"
        );
    }
}

/// **A sweep frozen part-way is thawed onto its row.** Stopped for fuel at
/// any point of `Update`, frozen as a save or a reload would, resumed into
/// the same program on a new host and run to its end, the machine finishes
/// the sweep it was in: every row once, in order.
#[test]
fn a_query_frozen_part_way_resumes_on_the_same_row() {
    let program = build(CENSUS);

    let mut stops = 0;
    for fuel in 1..=200 {
        let mut host = host_for(&program);
        let mut machine =
            Machine::new(&program, "Census.Update", &[Value::Float(0.1)]).expect("Update");
        match machine.run(&program, &mut host, fuel) {
            Run::Completed => break,
            Run::Suspended(Suspension::OutOfFuel) => {}
            other => panic!("fuel {fuel}: {other:?}"),
        }
        stops += 1;
        host.end_frame();
        let frozen = machine
            .freeze(&program, PendingBody::Update)
            .unwrap_or_else(|| panic!("a sweep stopped at fuel {fuel} freezes"));

        let (mut thawed, _) = resume(&frozen, program.fingerprint(), &program)
            .unwrap_or_else(|abandoned| panic!("fuel {fuel}: abandoned {abandoned:?}"));
        let mut next = Host::new()
            .with_fields(host.fields.clone())
            .for_entity(SUBJECT);
        next.components = FrameComponents::new(Arc::new(projection()));
        assert_eq!(
            thawed.run(&program, &mut next, u64::MAX),
            Run::Completed,
            "fuel {fuel}"
        );
        assert_eq!(
            total(&program, &next),
            Some(VISITED),
            "every row once, in order, thawed after fuel {fuel}"
        );
    }
    assert!(stops > 6, "the sweep stopped only {stops} times");
}
