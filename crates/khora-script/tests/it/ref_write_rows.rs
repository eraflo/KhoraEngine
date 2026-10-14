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

//! A system's `ref` binding is written back, row by row, as batches.
//!
//! At a row's end — where `continue` lands, and before a `break` or a
//! `return` leaves the sweep — the fields of a `ref` copy that differ from
//! the row as it was read go into the frame's working set at once, so every
//! later read sees them, and into the run's pending batch for that component
//! and those fields. A batch leaves as one `WorldCommand::WriteColumn` before
//! any later command of the run touching its component, and at the end of
//! every run — completed or stopped for fuel — so none outlives the run that
//! wrote it, and the run's writes keep the order they were made in.

use std::collections::HashMap;
use std::sync::Arc;

use crate::support::fresh_store;
use khora_core::ecs::entity::EntityId;
use khora_core::script::{
    ColumnSnapshot, ComponentName, ComponentProjection, FrameComponents, PendingBody,
    ProjectedComponent, ScriptValue, WorldCommand,
};
use khora_script::arena::Persisted;
use khora_script::vm::instruction::Instruction;
use khora_script::vm::{resume, Machine, Program, Run, Suspension, Value};
use khora_script::{compile_module, Host, MemoryLoader, ENGINE_COMPONENTS_MODULE};

/// The system's singleton entity: it holds none of the queried components.
const SINGLETON: EntityId = EntityId {
    index: 1,
    generation: 1,
};

/// Entity `10 * n`, generation 1.
const fn entity(n: u32) -> EntityId {
    EntityId {
        index: n * 10,
        generation: 1,
    }
}

/// The six holders of `Health`, in entity order: `entity(n)` holds
/// `current: n`, `max: 10`.
const HOLDERS: [u32; 6] = [1, 2, 3, 4, 5, 6];

const ENGINE: &str = "
component Health { int current; int max; }
component Regen { float rate; }
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

fn health(current: i64, max: i64) -> ScriptValue {
    struct_of(&[
        ("current", ScriptValue::Int(current)),
        ("max", ScriptValue::Int(max)),
    ])
}

/// Component `name` held by `holders`, one column listed out of entity order.
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

/// The frame's projection: `Health` on the six holders (stored in reverse),
/// `Regen` on the even ones.
fn projection() -> ComponentProjection {
    let healths: Vec<(EntityId, ScriptValue)> = HOLDERS
        .iter()
        .rev()
        .map(|&n| (entity(n), health(i64::from(n), 10)))
        .collect();
    let regens: Vec<(EntityId, ScriptValue)> = HOLDERS
        .iter()
        .filter(|&&n| n % 2 == 0)
        .map(|&n| (entity(n), struct_of(&[("rate", ScriptValue::Float(1.0))])))
        .collect();
    ComponentProjection::new(vec![
        projected("Health", &healths, health(0, 0)),
        projected(
            "Regen",
            &regens,
            struct_of(&[("rate", ScriptValue::Float(0.0))]),
        ),
    ])
}

/// A host for `system`'s instance on its singleton, over a fresh working set.
fn host_for(program: &Program, system: &str) -> Host {
    let mut host = Host::new()
        .with_fields(fresh_store(program, system))
        .for_entity(SINGLETON);
    host.components = FrameComponents::new(Arc::new(projection()));
    host
}

/// Runs `function` whole on `host`.
fn run_whole(program: &Program, function: &str, host: &mut Host) {
    let mut machine =
        Machine::new(program, function, &[Value::Float(0.1)]).expect("the function exists");
    assert_eq!(
        machine.run(program, host, u64::MAX),
        Run::Completed,
        "`{function}` completes"
    );
}

/// One column write: its component, its fields, its rows.
#[derive(Debug, Clone, PartialEq)]
struct Written {
    component: String,
    fields: Vec<String>,
    rows: Vec<(EntityId, Vec<ScriptValue>)>,
}

/// The column writes among `commands`, in order.
fn column_writes(commands: &[WorldCommand]) -> Vec<Written> {
    commands
        .iter()
        .filter_map(|command| match command {
            WorldCommand::WriteColumn { component, batch } => Some(Written {
                component: component.as_str().to_owned(),
                fields: batch.fields().to_vec(),
                rows: batch
                    .rows()
                    .map(|(entity, values)| (entity, values.to_vec()))
                    .collect(),
            }),
            _ => None,
        })
        .collect()
}

/// `entity`'s `Health.<field>` as the working set holds it.
fn working(host: &Host, entity: EntityId, field: &str) -> Option<i64> {
    host.components
        .get("Health", entity)?
        .as_fields()?
        .iter()
        .find(|(name, _)| name == field)?
        .1
        .as_int()
}

/// Field `name` of `system`'s instance.
fn field(program: &Program, host: &Host, system: &str, name: &str) -> Option<i64> {
    let slot = program.layout(system)?.slot_of(name)?;
    match host.fields.get(slot) {
        Some(Persisted::Scalar(Value::Int(value))) => Some(*value),
        _ => None,
    }
}

/// `system Sweeper { <members> }`.
fn sweeper(members: &str) -> Program {
    build(&format!("system Sweeper {{ int seen = 0; {members} }}"))
}

const INCREMENT: &str =
    "void Update(float dt) { foreach (ref Health h) { h.current = h.current + 1; } }";

// ─── One batch per component and fields ─────────────────────────────────────

/// **A sweep's writes leave as one column write**: six rows each adding one to
/// `current` are one `WriteColumn` of `Health`, writing `current` alone, its
/// rows in entity order with the values written — and no `Set`.
#[test]
fn ref_writes_leave_as_one_column_write_of_the_written_field() {
    let program = sweeper(INCREMENT);
    let mut host = host_for(&program, "Sweeper");
    run_whole(&program, "Sweeper.Update", &mut host);

    let commands = host.commands.as_slice().to_vec();
    assert_eq!(
        column_writes(&commands),
        vec![Written {
            component: "Health".to_owned(),
            fields: vec!["current".to_owned()],
            rows: HOLDERS
                .iter()
                .map(|&n| (entity(n), vec![ScriptValue::Int(i64::from(n) + 1)]))
                .collect(),
        }]
    );
    assert_eq!(
        commands.len(),
        1,
        "the batch and nothing else: {commands:?}"
    );
}

/// **Only what changed is written**: a row whose copy the body left as it
/// was read is not in the batch, and a field assigned the value it held is
/// not a write — a sweep changing nothing queues nothing.
#[test]
fn only_the_rows_and_fields_a_sweep_changed_are_written() {
    let program = sweeper(
        "void Update(float dt) {
             foreach (ref Health h) { if (h.current > 3) { h.current += 10; } h.max = 10; }
         }",
    );
    let mut host = host_for(&program, "Sweeper");
    run_whole(&program, "Sweeper.Update", &mut host);

    assert_eq!(
        column_writes(host.commands.as_slice()),
        vec![Written {
            component: "Health".to_owned(),
            fields: vec!["current".to_owned()],
            rows: [4, 5, 6]
                .into_iter()
                .map(|n| (entity(n), vec![ScriptValue::Int(i64::from(n) + 10)]))
                .collect(),
        }],
        "rows 1–3 changed nothing, and `max` was given what it held"
    );

    let unchanged = sweeper("void Update(float dt) { foreach (ref Health h) { h.max = 10; } }");
    let mut host = host_for(&unchanged, "Sweeper");
    run_whole(&unchanged, "Sweeper.Update", &mut host);
    assert!(
        host.commands.is_empty(),
        "nothing changed, nothing queued: {:?}",
        host.commands.as_slice()
    );
}

/// **Rows writing other fields leave in a batch of their own**: one batch per
/// component and set of written fields, each row in exactly one of them.
#[test]
fn rows_writing_other_fields_leave_in_a_batch_of_their_own() {
    let program = sweeper(
        "void Update(float dt) {
             foreach (ref Health h) { h.current += 1; if (h.current > 4) { h.max = 20; } }
         }",
    );
    let mut host = host_for(&program, "Sweeper");
    run_whole(&program, "Sweeper.Update", &mut host);

    let mut writes = column_writes(host.commands.as_slice());
    for write in &mut writes {
        write.fields.sort();
    }
    writes.sort_by_key(|write| write.fields.len());
    assert_eq!(writes.len(), 2, "one batch per set of fields: {writes:?}");
    assert_eq!(
        writes[0],
        Written {
            component: "Health".to_owned(),
            fields: vec!["current".to_owned()],
            rows: [1, 2, 3]
                .into_iter()
                .map(|n| (entity(n), vec![ScriptValue::Int(i64::from(n) + 1)]))
                .collect(),
        }
    );
    assert_eq!(writes[1].component, "Health");
    assert_eq!(
        writes[1].fields,
        vec!["current".to_owned(), "max".to_owned()]
    );
    assert_eq!(
        writes[1].rows.iter().map(|(e, _)| *e).collect::<Vec<_>>(),
        vec![entity(4), entity(5), entity(6)]
    );
}

// ─── The working set ────────────────────────────────────────────────────────

/// **A later read of the same run sees a row's write**: a second sweep over
/// `Health` after the first, and a `Get` of an earlier row from a later
/// row's body, read the written values.
#[test]
fn later_reads_in_the_same_run_see_a_rows_write() {
    let program = build(
        "system Sweeper {
             int total = 0;
             int earlier = -1;
             void Update(float dt) {
                 Entity? first = null;
                 foreach (Entity e, ref Health h) {
                     if (var f = first) {
                         if (earlier < 0) { earlier = f.Get(Health)?.current ?? -2; }
                     } else {
                         first = e;
                     }
                     h.current = h.current + 100;
                 }
                 foreach (Health k) { total += k.current; }
             }
         }",
    );
    let mut host = host_for(&program, "Sweeper");
    run_whole(&program, "Sweeper.Update", &mut host);

    assert_eq!(
        field(&program, &host, "Sweeper", "earlier"),
        Some(101),
        "the second row read the first row's write"
    );
    assert_eq!(
        field(&program, &host, "Sweeper", "total"),
        Some((1..=6).map(|n| n + 100).sum()),
        "the second sweep read every write of the first"
    );
}

/// **The scripts that run after see the write**: the working set holds it
/// once the system's run is over, and a behavior run next on the same frame
/// reads it.
#[test]
fn the_scripts_running_after_a_system_see_its_writes() {
    let program = build(&format!(
        "system Sweeper {{ {INCREMENT} }}
         behavior Census {{
             int total = 0;
             void Update(float dt) {{ foreach (Health h) {{ total += h.current; }} }}
         }}"
    ));
    let mut host = host_for(&program, "Sweeper");
    run_whole(&program, "Sweeper.Update", &mut host);

    for n in HOLDERS {
        assert_eq!(
            working(&host, entity(n), "current"),
            Some(i64::from(n) + 1),
            "entity {n}'s write is in the working set"
        );
    }

    host.fields = fresh_store(&program, "Census");
    host.entity = Some(entity(99));
    run_whole(&program, "Census.Update", &mut host);
    assert_eq!(
        field(&program, &host, "Census", "total"),
        Some((1..=6).map(|n| n + 1).sum()),
        "the behavior read the system's writes"
    );
}

// ─── Leaving a sweep ────────────────────────────────────────────────────────

/// **`break` and `return` keep the row's write**; `continue` lands at the
/// row's end, so every row it skips the rest of is written.
#[test]
fn leaving_a_sweep_keeps_the_current_rows_write() {
    let breaking =
        sweeper("void Update(float dt) { foreach (ref Health h) { h.current = 50; break; } }");
    let mut host = host_for(&breaking, "Sweeper");
    run_whole(&breaking, "Sweeper.Update", &mut host);
    assert_eq!(
        column_writes(host.commands.as_slice())
            .into_iter()
            .flat_map(|write| write.rows)
            .collect::<Vec<_>>(),
        vec![(entity(1), vec![ScriptValue::Int(50)])],
        "`break`"
    );
    assert_eq!(working(&host, entity(1), "current"), Some(50));

    let returning = sweeper(
        "void Update(float dt) { seen = Stop(); }
         int Stop() { foreach (ref Health h) { h.current = 60; return 1; } return 0; }",
    );
    let mut host = host_for(&returning, "Sweeper");
    run_whole(&returning, "Sweeper.Update", &mut host);
    assert_eq!(
        column_writes(host.commands.as_slice())
            .into_iter()
            .flat_map(|write| write.rows)
            .collect::<Vec<_>>(),
        vec![(entity(1), vec![ScriptValue::Int(60)])],
        "`return`"
    );

    let continuing =
        sweeper("void Update(float dt) { foreach (ref Health h) { h.current = 70; continue; } }");
    let mut host = host_for(&continuing, "Sweeper");
    run_whole(&continuing, "Sweeper.Update", &mut host);
    assert_eq!(
        column_writes(host.commands.as_slice())
            .into_iter()
            .flat_map(|write| write.rows)
            .map(|(entity, _)| entity)
            .collect::<Vec<_>>(),
        HOLDERS.iter().map(|&n| entity(n)).collect::<Vec<_>>(),
        "`continue`"
    );
}

// ─── Order among the run's commands ─────────────────────────────────────────

/// **A batch leaves before a later command on its component.** Each row's
/// body `Set`s the previous row's `Health` to 9 after that row wrote 5: the
/// previous row's column write comes before the `Set` among the commands, so
/// applying them in order leaves 9 on every row but the last, which no later
/// `Set` overwrote.
#[test]
fn a_batch_leaves_before_a_later_command_on_its_component() {
    let program = sweeper(
        "void Update(float dt) {
             Entity? previous = null;
             foreach (Entity e, ref Health h) {
                 if (var p = previous) { p.Set(Health { current: 9 }); }
                 h.current = 5;
                 previous = e;
             }
         }",
    );
    let mut host = host_for(&program, "Sweeper");
    run_whole(&program, "Sweeper.Update", &mut host);

    // The commands applied in order, as the frame boundary applies them.
    let mut current: HashMap<EntityId, i64> =
        HOLDERS.iter().map(|&n| (entity(n), i64::from(n))).collect();
    let commands = host.commands.as_slice().to_vec();
    for command in &commands {
        match command {
            WorldCommand::WriteColumn { component, batch } if component.as_str() == "Health" => {
                let slot = batch
                    .fields()
                    .iter()
                    .position(|field| field == "current")
                    .expect("the batch writes `current`");
                for (entity, values) in batch.rows() {
                    current.insert(entity, values[slot].as_int().expect("an int"));
                }
            }
            WorldCommand::SetComponent {
                entity,
                component,
                value,
            } if component.as_str() == "Health" => {
                if let Some(written) = value
                    .as_fields()
                    .and_then(|fields| fields.iter().find(|(name, _)| name == "current"))
                {
                    current.insert(*entity, written.1.as_int().expect("an int"));
                }
            }
            _ => {}
        }
    }
    let expected: HashMap<EntityId, i64> = HOLDERS
        .iter()
        .map(|&n| (entity(n), if n == 6 { 5 } else { 9 }))
        .collect();
    assert_eq!(current, expected, "in the order written: {commands:?}");
    assert!(
        column_writes(&commands).len() >= 2,
        "a batch left before each `Set`, not one at the end: {commands:?}"
    );
}

// ─── Short of fuel ──────────────────────────────────────────────────────────

/// **A sweep short of fuel writes each row once, and no batch outlives a
/// run.** Run after run on `slice` fuel, the rows written by each run leave
/// with it: every row the working set shows written has left as a column
/// write by the end of the run that wrote it, and over the whole sweep each
/// row is written exactly once, with its value.
#[test]
fn a_sweep_short_of_fuel_writes_each_row_once_and_leaves_no_batch_behind() {
    let program = sweeper(INCREMENT);
    let mut split = 0;
    for slice in 1..=40 {
        let mut host = host_for(&program, "Sweeper");
        let mut machine =
            Machine::new(&program, "Sweeper.Update", &[Value::Float(0.1)]).expect("Update");
        let mut written: Vec<(EntityId, Vec<ScriptValue>)> = Vec::new();
        let mut runs_writing = 0;
        let mut runs = 0;
        loop {
            runs += 1;
            assert!(runs < 10_000, "no end at slice {slice}");
            let outcome = machine.run(&program, &mut host, slice);
            let commands = host.end_frame();
            let rows: Vec<_> = column_writes(commands.as_slice())
                .into_iter()
                .flat_map(|write| write.rows)
                .collect();
            if !rows.is_empty() {
                runs_writing += 1;
            }
            written.extend(rows);
            for n in HOLDERS {
                if working(&host, entity(n), "current") == Some(i64::from(n) + 1) {
                    assert!(
                        written.iter().any(|(e, _)| *e == entity(n)),
                        "slice {slice}: row {n} is written in the working set, and its batch did not leave with the run"
                    );
                }
            }
            match outcome {
                Run::Completed => break,
                Run::Suspended(Suspension::OutOfFuel) => {}
                other => panic!("slice {slice}: {other:?}"),
            }
        }
        if runs_writing > 1 {
            split += 1;
        }
        let mut rows = written.clone();
        rows.sort_by_key(|(entity, _)| entity.index);
        assert_eq!(
            rows,
            HOLDERS
                .iter()
                .map(|&n| (entity(n), vec![ScriptValue::Int(i64::from(n) + 1)]))
                .collect::<Vec<_>>(),
            "slice {slice}: every row once, with its value — written {written:?}"
        );
    }
    assert!(split > 0, "no slice split the sweep's writes over runs");
}

/// **A machine frozen mid-sweep holds no batch**: what the rows before the
/// stop wrote left with the run, and the thawed machine, on a new frame,
/// writes the rest — each row once over the two.
#[test]
fn a_sweep_frozen_part_way_leaves_its_rows_written_once() {
    let program = sweeper(INCREMENT);
    let mut stops = 0;
    for fuel in 1..=400 {
        let mut host = host_for(&program, "Sweeper");
        let mut machine =
            Machine::new(&program, "Sweeper.Update", &[Value::Float(0.1)]).expect("Update");
        match machine.run(&program, &mut host, fuel) {
            Run::Completed => break,
            Run::Suspended(Suspension::OutOfFuel) => {}
            other => panic!("fuel {fuel}: {other:?}"),
        }
        stops += 1;
        let before: Vec<_> = column_writes(host.end_frame().as_slice())
            .into_iter()
            .flat_map(|write| write.rows)
            .collect();
        let frozen = machine
            .freeze(&program, PendingBody::Update)
            .unwrap_or_else(|| panic!("a sweep stopped at fuel {fuel} freezes"));

        let (mut thawed, _) = resume(&frozen, program.fingerprint(), &program)
            .unwrap_or_else(|abandoned| panic!("fuel {fuel}: abandoned {abandoned:?}"));
        let mut next = Host::new()
            .with_fields(host.fields.clone())
            .for_entity(SINGLETON);
        next.components = FrameComponents::new(Arc::new(projection()));
        assert_eq!(
            thawed.run(&program, &mut next, u64::MAX),
            Run::Completed,
            "fuel {fuel}"
        );
        let after: Vec<_> = column_writes(next.commands.as_slice())
            .into_iter()
            .flat_map(|write| write.rows)
            .collect();

        let mut rows: Vec<EntityId> = before.iter().chain(&after).map(|(e, _)| *e).collect();
        rows.sort_by_key(|entity| entity.index);
        assert_eq!(
            rows,
            HOLDERS.iter().map(|&n| entity(n)).collect::<Vec<_>>(),
            "fuel {fuel}: each row once — before the freeze {before:?}, after {after:?}"
        );
    }
    assert!(stops > 6, "the sweep stopped only {stops} times");
}

// ─── The instruction ────────────────────────────────────────────────────────

/// The `WriteRow`s of the function named `name`.
fn row_writes(program: &Program, name: &str) -> Vec<(String, u16)> {
    program
        .function(name)
        .unwrap_or_else(|| panic!("`{name}` is a function"))
        .code
        .iter()
        .filter_map(|instruction| match instruction {
            Instruction::WriteRow {
                component, count, ..
            } => Some((
                program
                    .string(*component)
                    .unwrap_or("<no string>")
                    .to_owned(),
                *count,
            )),
            _ => None,
        })
        .collect()
}

/// **A `ref` binding compiles to a row write naming its component**, with
/// the component's field count; a read-only binding writes nothing back.
#[test]
fn a_ref_binding_compiles_a_row_write_naming_its_component() {
    let program = sweeper(INCREMENT);
    let writes = row_writes(&program, "Sweeper.Update");
    assert!(!writes.is_empty(), "a row write for `ref Health h`");
    assert!(
        writes
            .iter()
            .all(|write| *write == ("Health".to_owned(), 2)),
        "{writes:?}"
    );

    let reading = sweeper("void Update(float dt) { foreach (Health h) { seen += h.current; } }");
    assert!(
        row_writes(&reading, "Sweeper.Update").is_empty(),
        "a read-only binding is not written back"
    );
}

/// **A row write costs one plus the fields it compares.**
#[test]
fn a_row_write_costs_one_plus_its_fields() {
    for count in [0u16, 1, 2, 7] {
        let write = Instruction::WriteRow {
            entity: 0,
            copy: 1,
            original: 2,
            component: 0,
            count,
        };
        assert_eq!(write.cost(), 1 + u64::from(count), "{count} fields");
    }
}

/// **A row write's fingerprint names its component, not where its name
/// sits**: a literal added before the system moves `Health` to another slot
/// of the string table, and `Update` keeps its fingerprint.
#[test]
fn a_row_writes_fingerprint_names_its_component_not_its_string_slot() {
    let plain = sweeper(INCREMENT);
    let moved = build(&format!(
        "fn string Noise() {{ return \"aaa\"; }}
         system Sweeper {{ int seen = 0; {INCREMENT} }}"
    ));
    assert!(
        !row_writes(&plain, "Sweeper.Update").is_empty(),
        "the sweep writes its rows back"
    );
    let slot = |program: &Program| {
        program
            .strings
            .iter()
            .position(|string| string == "Health")
            .expect("`Health` is a string of the program")
    };
    assert_ne!(slot(&plain), slot(&moved), "the literal moved `Health`");
    let fingerprint = |program: &Program| {
        program
            .function("Sweeper.Update")
            .expect("`Update`")
            .fingerprint
    };
    assert_eq!(fingerprint(&plain), fingerprint(&moved));
}
