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

//! `Spawn(bullet, C { … }, …, at: p, facing: q)` — a prefab spawned in the
//! turn — and `e.SetMaterial(m)`.
//!
//! A first argument of type `Prefab` or `Prefab?` selects the prefab form:
//! then zero or more component literals, `at:` (a `Vec3`, required) and
//! `facing:` (a `Quat`, optional). It is the instance root's `Entity`,
//! reserved in the turn like a plain spawn's. The components it writes merge
//! onto the root's when the boundary applies it; in the turn, they are what
//! the working set holds of the root.
//!
//! A `null` prefab faults the turn, and so does one the host's catalog does
//! not hold; a host given no catalog accepts any. `e.SetMaterial(m)` takes a
//! `Material` or a `Material?`, and a `null` faults.

mod checks;
mod commands;
mod costs;
mod materials;

use std::collections::HashMap;
use std::sync::Arc;

use khora_core::asset::AssetUUID;
use khora_core::ecs::entity::EntityId;
use khora_core::script::{
    ColumnSnapshot, ComponentName, ComponentProjection, FrameComponents, ProjectedComponent,
    ScriptValue, WorldCommand,
};
use khora_script::vm::{Machine, Program, Run, Value};
use khora_script::{
    compile_module, CompileOutcome, Diagnostic, Host, MemoryLoader, ENGINE_COMPONENTS_MODULE,
};

/// The entity a test's functions act on.
const SUBJECT: EntityId = EntityId {
    index: 4,
    generation: 1,
};

/// The engine module, as the mirror would serve it.
const ENGINE: &str = "
component Health {
    int current;
    int max;
}
component Collider {
    float friction;
    bool is_sensor;
}
";

/// The source `main` is compiled as: the import, then `main`.
fn as_compiled(main: &str) -> String {
    format!("import \"{ENGINE_COMPONENTS_MODULE}\";\n{main}")
}

fn compile(main: &str) -> CompileOutcome {
    let loader = MemoryLoader::new()
        .with(ENGINE_COMPONENTS_MODULE, ENGINE)
        .with("main.erg", as_compiled(main));
    compile_module(&loader, "main.erg")
}

fn errors(result: &CompileOutcome) -> Vec<&Diagnostic> {
    result.diagnostics.iter().filter(|d| d.is_error()).collect()
}

/// A diagnostic's message and note, as one line.
fn said(diagnostic: &Diagnostic) -> String {
    format!(
        "{} | {}",
        diagnostic.message,
        diagnostic.note.as_deref().unwrap_or_default()
    )
}

fn underlined<'s>(source: &'s str, diagnostic: &Diagnostic) -> &'s str {
    source
        .get(diagnostic.span.start as usize..diagnostic.span.end as usize)
        .unwrap_or("<out of the file>")
}

fn listed(main: &str, result: &CompileOutcome) -> Vec<String> {
    let source = as_compiled(main);
    errors(result)
        .into_iter()
        .map(|d| format!("{} @ {:?}", said(d), underlined(&source, d)))
        .collect()
}

/// Compiles `main`, which must compile without an error.
fn build(main: &str) -> Program {
    let result = compile(main);
    let found = listed(main, &result);
    assert!(found.is_empty(), "should compile:\n{main}\n{found:?}");
    result.program.expect("a program with no errors")
}

/// The refusal of `main` underlining (within) the last `place` written in it,
/// saying every fragment of `fragments` — each a `|`-separated set of
/// alternatives.
fn refused_at(main: &str, place: &str, fragments: &[&str]) -> Diagnostic {
    let result = compile(main);
    assert!(
        !result.succeeded(),
        "should be refused, and compiled:\n{main}"
    );
    let source = as_compiled(main);
    let start = source
        .rfind(place)
        .unwrap_or_else(|| panic!("{place:?} is written in\n{main}"));
    let end = start + place.len();
    errors(&result)
        .into_iter()
        .find(|d| {
            let text = said(d);
            fragments
                .iter()
                .all(|fragment| fragment.split('|').any(|alt| text.contains(alt)))
                && d.span.start as usize >= start
                && d.span.end as usize <= end
                && d.span.start < d.span.end
        })
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "expected a refusal of\n{main}\nsaying {fragments:?} at `{place}`; got {:?}",
                listed(main, &result)
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

/// A component the program reads, held by nobody, with `default` as its
/// default row.
fn read_by_nobody(name: &str, default: ScriptValue) -> ProjectedComponent {
    ProjectedComponent::new(
        ComponentName::new(name),
        vec![Box::new(Rows(Vec::new()))],
        HashMap::new(),
        Some(default),
    )
}

/// A host over a working set reading `Health` and `Collider`, neither held by
/// anyone: whatever an entity holds in it, this frame's commands gave it. It
/// is given no prefab catalog.
fn host() -> Host {
    let mut host = Host::new().for_entity(SUBJECT);
    host.components = FrameComponents::new(Arc::new(ComponentProjection::new(vec![
        read_by_nobody(
            "Health",
            fields(&[
                ("current", ScriptValue::Int(100)),
                ("max", ScriptValue::Int(100)),
            ]),
        ),
        read_by_nobody(
            "Collider",
            fields(&[
                ("friction", ScriptValue::Float(0.5)),
                ("is_sensor", ScriptValue::Bool(false)),
            ]),
        ),
    ])));
    host
}

/// The bullet prefab's identity.
fn bullet() -> AssetUUID {
    AssetUUID::from_bytes([
        0x1a, 0x2b, 0x3c, 0x4d, 0x5e, 0x6f, 0x40, 0x81, 0x92, 0xa3, 0xb4, 0xc5, 0xd6, 0xe7, 0xf8,
        0x09,
    ])
}

/// Another prefab's identity.
fn other() -> AssetUUID {
    AssetUUID::from_bytes([0xee; 16])
}

/// How a run of a function ended, with what it returned and queued.
struct Ran {
    run: Run,
    result: Value,
    commands: Vec<WorldCommand>,
}

/// Runs `function` of `program` on `args`, whole, on `host`.
fn run_on(program: &Program, function: &str, args: &[Value], mut host: Host) -> Ran {
    let mut machine = Machine::new(program, function, args).unwrap_or_else(|| {
        panic!(
            "`{function}` is a function taking {} argument(s)",
            args.len()
        )
    });
    let run = machine.run(program, &mut host, u64::MAX);
    Ran {
        result: machine.result(),
        run,
        commands: host.commands.as_slice().to_vec(),
    }
}

/// Runs `function` of `program` on `args` on a fresh [`host`]: it must
/// complete. What it returned, and what it queued.
fn run(program: &Program, function: &str, args: &[Value]) -> (Value, Vec<WorldCommand>) {
    let ran = run_on(program, function, args, host());
    assert_eq!(ran.run, Run::Completed, "`{function}` completes");
    (ran.result, ran.commands)
}

/// The fault a run ended on, written out as a running problem shows it.
fn fault_of(ran: &Ran) -> String {
    match &ran.run {
        Run::Faulted(fault) => format!("{fault:?}"),
        other => panic!("the run faults, and it ended {other:?}"),
    }
}

fn entity_of(value: &Value) -> EntityId {
    match value {
        Value::Entity(entity) => *entity,
        other => panic!("an entity, not {other:?}"),
    }
}

/// The roots the prefab spawns among `commands` carry, in order.
fn prefab_roots(commands: &[WorldCommand]) -> Vec<EntityId> {
    commands
        .iter()
        .filter_map(|command| match command {
            WorldCommand::SpawnPrefab { entity, .. } => Some(*entity),
            _ => None,
        })
        .collect()
}
