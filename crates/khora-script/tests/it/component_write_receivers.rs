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

//! The receiver of an entity method, whatever expression yields the entity,
//! and the values a component write carries, whatever expression yields them.

use khora_core::ecs::entity::EntityId;
use khora_core::math::Vec3;
use khora_core::script::{ComponentName, ScriptValue, WorldCommand};
use khora_script::vm::{Machine, Program, Run, Value};
use khora_script::{
    compile_module, CompileOutcome, Diagnostic, Host, MemoryLoader, ENGINE_COMPONENTS_MODULE,
};

const SUBJECT: EntityId = EntityId {
    index: 4,
    generation: 1,
};

const OTHER: EntityId = EntityId {
    index: 8,
    generation: 3,
};

const ENGINE: &str = "
component Health {
    int current;
    int max;
}
component Label {
    string text;
    int[] counts;
    Pose pose;
    Vec3? aim;
}
struct Pose {
    Vec3 at;
    string name;
}
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

fn build(main: &str) -> Program {
    let result = compile(main);
    let errors: Vec<&Diagnostic> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "should compile: {errors:?}");
    result.program.expect("a program with no errors")
}

fn queued(program: &Program, function: &str, args: &[Value]) -> Vec<WorldCommand> {
    let mut host = Host::new().for_entity(SUBJECT);
    let mut machine = Machine::new(program, function, args).expect("the function exists");
    assert_eq!(machine.run(program, &mut host, u64::MAX), Run::Completed);
    host.commands.as_slice().to_vec()
}

fn health(entity: EntityId, fields: &[(&str, i64)]) -> WorldCommand {
    WorldCommand::SetComponent {
        entity,
        component: ComponentName::new("Health"),
        value: ScriptValue::Struct(
            fields
                .iter()
                .map(|(name, value)| ((*name).to_owned(), ScriptValue::Int(*value)))
                .collect(),
        ),
    }
}

/// An `Entity?` narrowed by `if (var …)` is an `Entity`: its methods are
/// called on it like on any other.
#[test]
fn a_narrowed_optional_entity_takes_methods() {
    let program = build(
        "fn void Main(Entity? target) {
             if (var t = target) {
                 t.Set(Health { current: 3 });
                 t.Despawn();
             }
         }",
    );

    assert_eq!(
        queued(&program, "Main", &[Value::Entity(OTHER)]),
        vec![
            health(OTHER, &[("current", 3)]),
            WorldCommand::Despawn { entity: OTHER },
        ]
    );
}

/// The receiver is any expression of type `Entity`: an element, a struct's
/// field, a call's result, a conditional.
#[test]
fn any_entity_expression_is_a_receiver() {
    let program = build(
        "struct Pair { Entity a; Entity b; }
         fn Entity Pick(Entity e) { return e; }
         fn void Main(Entity[] es, Pair p, bool flag) {
             es[1].Set(Health { current: 1 });
             p.b.Set(Health { current: 2 });
             Pick(es[0]).Set(Health { current: 3 });
             (flag ? p.a : p.b).Set(Health { current: 4 });
         }",
    );

    let mut host = Host::new().for_entity(SUBJECT);
    let entities = host
        .arena
        .alloc(khora_script::arena::Object::Array(vec![
            Value::Entity(SUBJECT),
            Value::Entity(OTHER),
        ]))
        .expect("room for two");
    let pair = program
        .structs
        .iter()
        .position(|layout| layout.name == "Pair")
        .expect("Pair is laid out");
    let pair = host
        .arena
        .alloc(khora_script::arena::Object::Struct {
            layout: pair as u16,
            fields: vec![Value::Entity(SUBJECT), Value::Entity(OTHER)],
        })
        .expect("room for a pair");
    let mut machine = Machine::new(
        &program,
        "Main",
        &[
            Value::Obj(khora_script::vm::value::ObjRef::Arena(entities)),
            Value::Obj(khora_script::vm::value::ObjRef::Arena(pair)),
            Value::Bool(true),
        ],
    )
    .expect("Main exists");
    assert_eq!(machine.run(&program, &mut host, u64::MAX), Run::Completed);

    assert_eq!(
        host.commands.as_slice(),
        &[
            health(OTHER, &[("current", 1)]),
            health(OTHER, &[("current", 2)]),
            health(SUBJECT, &[("current", 3)]),
            health(SUBJECT, &[("current", 4)]),
        ]
    );
}

/// A field's value may itself write a component: that write is queued first,
/// while the outer write's values are being computed, and the outer write
/// still carries its own values.
#[test]
fn a_write_inside_a_field_value_comes_first() {
    let program = build(
        "fn int Bump(Entity e) { e.Set(Health { max: 9 }); return 4; }
         fn void Main(Entity e) { e.Set(Health { max: 7, current: Bump(e) }); }",
    );

    assert_eq!(
        queued(&program, "Main", &[Value::Entity(OTHER)]),
        vec![
            health(OTHER, &[("max", 9)]),
            health(OTHER, &[("max", 7), ("current", 4)]),
        ]
    );
}

/// Text built at run time, an array, a struct and a `null` all cross the
/// boundary as the values the script made.
#[test]
fn a_write_carries_built_text_arrays_structs_and_null() {
    let program = build(
        r#"fn string Word(int n) { if (n > 2) { return "big"; } return "small"; }
           fn void Main(Entity e, int n) {
               e.Set(Label { text: "size: " + Word(n), counts: [n, n + 1], pose: Pose { at: Vec3(1.0, 2.0, 3.0), name: "p" + Word(1) }, aim: null });
           }"#,
    );

    assert_eq!(
        queued(&program, "Main", &[Value::Entity(OTHER), Value::Int(5)]),
        vec![WorldCommand::SetComponent {
            entity: OTHER,
            component: ComponentName::new("Label"),
            value: ScriptValue::Struct(vec![
                ("text".to_owned(), ScriptValue::Str("size: big".to_owned())),
                (
                    "counts".to_owned(),
                    ScriptValue::Array(vec![ScriptValue::Int(5), ScriptValue::Int(6)])
                ),
                (
                    "pose".to_owned(),
                    ScriptValue::Struct(vec![
                        ("at".to_owned(), ScriptValue::Vec3(Vec3::new(1.0, 2.0, 3.0))),
                        ("name".to_owned(), ScriptValue::Str("psmall".to_owned())),
                    ])
                ),
                ("aim".to_owned(), ScriptValue::Null),
            ]),
        }]
    );
}

/// The rewrite offered for the free form is a program that means the same:
/// a receiver written with an operator is parenthesised.
#[test]
fn the_rewrite_of_an_operator_receiver_keeps_its_meaning() {
    let result =
        compile("fn void Main(Entity? a, Entity b) { SetPosition(a ?? b, Vec3(0.0, 0.0, 0.0)); }");
    let said: Vec<String> = result
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| d.message.clone())
        .collect();
    assert!(
        said.iter()
            .any(|message| message.contains("Write `(a ?? b).SetPosition(")),
        "the rewrite must call the method on `a ?? b`, not on `b`; got {said:?}"
    );
}
