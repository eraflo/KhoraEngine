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

//! What a program says about the components its scripts declare (Ergon spec
//! 08): `Program::declared`, one entry per `component` written outside the
//! engine's module — its module, its domain, and each field's type and
//! default — sorted by name. This is what the engine registers.

use std::collections::HashMap;
use std::sync::Arc;

use khora_core::ecs::entity::EntityId;
use khora_core::ecs::SemanticDomain;
use khora_core::script::{
    ComponentName, ComponentProjection, ErgonType, FrameComponents, ProjectedComponent, ScriptValue,
};
use khora_script::vm::{
    DeclaredComponent, DeclaredField, DeclaredKind, Machine, Program, Run, Value,
};
use khora_script::{
    compile_module, CompileOutcome, Diagnostic, Host, MemoryLoader, ENGINE_COMPONENTS_MODULE,
};

const SUBJECT: EntityId = EntityId {
    index: 4,
    generation: 1,
};

/// The engine module, as the mirror would serve it.
const ENGINE: &str = "
component RigidBody {
    float mass;
    bool ccd_enabled;
}
component Collider {
    float friction;
}
";

/// `main.erg` importing the engine module, beside `others`.
fn compile_with(main: &str, others: &[(&str, &str)]) -> CompileOutcome {
    let mut loader = MemoryLoader::new()
        .with(ENGINE_COMPONENTS_MODULE, ENGINE)
        .with(
            "main.erg",
            format!("import \"{ENGINE_COMPONENTS_MODULE}\";\n{main}"),
        );
    for &(path, source) in others {
        loader = loader.with(path, source);
    }
    compile_module(&loader, "main.erg")
}

fn build_with(main: &str, others: &[(&str, &str)]) -> Program {
    let result = compile_with(main, others);
    let errors: Vec<&Diagnostic> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "should compile:\n{main}\n{errors:?}");
    result.program.expect("a program with no errors")
}

fn build(main: &str) -> Program {
    build_with(main, &[])
}

fn field(name: &str, ty: ErgonType, default: ScriptValue) -> DeclaredField {
    DeclaredField {
        name: name.to_owned(),
        ty,
        default,
        hints: khora_core::script::FieldHints::default(),
    }
}

/// **The program lists what its scripts declare**: every `component` outside
/// the engine module, sorted by name, each naming the module that declares it
/// — imported or not — its domain (`Script` when none is written), and its
/// fields in declaration order with their types and defaults. The engine's own
/// components are not in the list: the engine registers those itself.
#[test]
fn a_program_lists_the_components_it_declares() {
    let program = build_with(
        "import \"gear/shield.erg\";
         component Loot { string item = \"\"; int value = 30; int[] tags; }
         fn void F(Entity e) { e.Set(RigidBody { mass: 1.0 }); e.Add(Shield { }); }",
        &[(
            "gear/shield.erg",
            "[Domain(Physics)]
             component Shield { int charge = 3; float? decay; bool raised = true; }",
        )],
    );

    assert_eq!(
        program.declared,
        vec![
            DeclaredComponent {
                name: "Loot".to_owned(),
                module: "main.erg".to_owned(),
                domain: SemanticDomain::Script,
                fields: vec![
                    field("item", ErgonType::Str, ScriptValue::Str(String::new())),
                    field("value", ErgonType::Int, ScriptValue::Int(30)),
                    field(
                        "tags",
                        ErgonType::Array(Box::new(ErgonType::Int)),
                        ScriptValue::Array(Vec::new()),
                    ),
                ],
                kind: DeclaredKind::Component,
                order: 0,
            },
            DeclaredComponent {
                name: "Shield".to_owned(),
                module: "gear/shield.erg".to_owned(),
                domain: SemanticDomain::Physics,
                fields: vec![
                    field("charge", ErgonType::Int, ScriptValue::Int(3)),
                    field(
                        "decay",
                        ErgonType::Optional(Box::new(ErgonType::Float)),
                        ScriptValue::Null,
                    ),
                    field("raised", ErgonType::Bool, ScriptValue::Bool(true)),
                ],
                kind: DeclaredKind::Component,
                order: 0,
            },
        ]
    );

    assert!(
        build("fn void F(Entity e) { e.Set(RigidBody { mass: 1.0 }); }")
            .declared
            .is_empty(),
        "a program declaring nothing lists nothing — not the engine's components"
    );
}

/// **Each `[Domain(X)]` reaches the declaration as that domain**, spelled the
/// way the engine names it.
#[test]
fn each_domain_name_reaches_the_declaration() {
    for domain in [
        SemanticDomain::Spatial,
        SemanticDomain::Render,
        SemanticDomain::Audio,
        SemanticDomain::Physics,
        SemanticDomain::Ui,
        SemanticDomain::Script,
    ] {
        let program = build(&format!(
            "[Domain({})]\ncomponent Health {{ int current = 100; }}",
            domain.name()
        ));
        assert_eq!(
            program
                .declared
                .iter()
                .map(|declared| declared.domain)
                .collect::<Vec<_>>(),
            vec![domain],
            "`[Domain({})]`",
            domain.name()
        );
    }
}

/// **A field without a default declares its type's zero** — the value a new
/// row starts with.
#[test]
fn a_field_without_a_default_declares_its_zero() {
    let program = build(
        "component Tally {
             int n;
             float f;
             bool b;
             string s;
             int? maybe;
             string[] names;
         }",
    );

    let tally = program
        .declared
        .iter()
        .find(|declared| declared.name == "Tally")
        .expect("Tally is declared");
    assert_eq!(
        tally
            .fields
            .iter()
            .map(|field| (field.name.as_str(), field.default.clone()))
            .collect::<Vec<_>>(),
        vec![
            ("n", ScriptValue::Int(0)),
            ("f", ScriptValue::Float(0.0)),
            ("b", ScriptValue::Bool(false)),
            ("s", ScriptValue::Str(String::new())),
            ("maybe", ScriptValue::Null),
            ("names", ScriptValue::Array(Vec::new())),
        ]
    );
}

/// The declarations come out sorted by name whatever order, and whichever
/// modules, they were written in.
#[test]
fn the_declarations_are_sorted_by_name() {
    let program = build_with(
        "import \"b.erg\";
         component Zebra { int n = 0; }
         component Apple { int n = 0; }",
        &[("b.erg", "component Mango { int n = 0; }")],
    );

    assert_eq!(
        program
            .declared
            .iter()
            .map(|declared| declared.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Apple", "Mango", "Zebra"]
    );
}

/// **`Add(Health { })` then `Get(Health)` reads the declared defaults** — the
/// engine's default row for a declared component is built from the
/// declaration, so the copy a script reads after its own `Add` holds them.
#[test]
fn add_then_get_reads_the_declared_defaults() {
    let program = build(
        "[Domain(Physics)]
         component Health { int current = 100; int max = 100; }
         fn int F(Entity e) {
             e.Add(Health { });
             if (var h = e.Get(Health)) { return h.current * 1000 + h.max; }
             return -1;
         }",
    );
    let health = program
        .declared
        .iter()
        .find(|declared| declared.name == "Health")
        .expect("Health is declared");
    // The default row the engine projects for it: the declared defaults.
    let default = ScriptValue::Struct(
        health
            .fields
            .iter()
            .map(|field| (field.name.clone(), field.default.clone()))
            .collect(),
    );
    let projection = ComponentProjection::new(vec![ProjectedComponent::new(
        ComponentName::new("Health"),
        Vec::new(),
        HashMap::new(),
        Some(default),
    )]);
    let mut host = Host::new().for_entity(SUBJECT);
    host.components = FrameComponents::new(Arc::new(projection));
    let mut machine =
        Machine::new(&program, "F", &[Value::Entity(SUBJECT)]).expect("the function exists");

    assert_eq!(machine.run(&program, &mut host, u64::MAX), Run::Completed);
    assert_eq!(machine.result(), Value::Int(100_100));
}

/// **A struct a script names `Value` is a struct**: the type a file keeps of
/// a field of it — written `Value{n:int}` — reads back as that struct, and a
/// component field of it is typed with the struct's fields, so a value of
/// another shape is kept out of it.
#[test]
fn a_field_of_a_struct_named_value_is_typed_as_that_struct() {
    let expected = ErgonType::Struct {
        name: "Value".to_owned(),
        fields: Some(vec![("n".to_owned(), ErgonType::Int)]),
    };
    assert_eq!(
        ErgonType::parse("Value{n:int}"),
        Some(expected.clone()),
        "the type a file keeps of the field reads back"
    );

    let program = build(
        "struct Value { int n = 0; }
         component Holder { Value held = Value { n: 1 }; }",
    );
    let holder = program
        .declared
        .iter()
        .find(|declared| declared.name == "Holder")
        .expect("`Holder` is declared");
    assert_eq!(holder.fields[0].ty, expected, "the struct, with its fields");
}
