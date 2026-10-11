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

//! A project declares its own components (Ergon spec 08).
//!
//! What the compiler accepts and refuses about a `component` written in a
//! script: its `[Domain(…)]`, its field defaults, the zero a field without one
//! takes, and a name the engine already uses. What the declarations become in
//! the program is `declared_components`; the `World` side is the engine's.

use std::collections::HashMap;
use std::sync::Arc;

use khora_core::ecs::entity::EntityId;
use khora_core::script::{
    ColumnSnapshot, ComponentName, ComponentProjection, FrameComponents, ProjectedComponent,
    ScriptValue,
};
use khora_script::arena::Owned;
use khora_script::modules::SourceLoader;
use khora_script::vm::{Machine, Program, Run, Value};
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

/// The name of every engine component — what `PreludeLoader` answers.
const ENGINE_NAMES: [&str; 3] = ["RigidBody", "Collider", "OldBody"];

/// A loader serving `modules` and the engine module, and naming the engine's
/// components the way the engine's own loader does.
struct EngineLoader {
    inner: MemoryLoader,
    names: Vec<String>,
}

impl EngineLoader {
    fn new(modules: &[(&str, &str)]) -> Self {
        let mut inner = MemoryLoader::new().with(ENGINE_COMPONENTS_MODULE, ENGINE);
        for &(path, source) in modules {
            inner = inner.with(path, source);
        }
        Self {
            inner,
            names: ENGINE_NAMES.iter().map(|name| (*name).to_owned()).collect(),
        }
    }
}

impl SourceLoader for EngineLoader {
    fn load(&self, path: &str) -> Option<String> {
        self.inner.load(path)
    }

    fn engine_components(&self) -> &[String] {
        &self.names
    }
}

/// `main.erg` alone (it does not import the engine module), beside `others`.
fn compile_with(main: &str, others: &[(&str, &str)]) -> CompileOutcome {
    let mut modules = vec![("main.erg", main)];
    modules.extend_from_slice(others);
    compile_module(&EngineLoader::new(&modules), "main.erg")
}

fn compile(main: &str) -> CompileOutcome {
    compile_with(main, &[])
}

fn errors_of(result: &CompileOutcome) -> Vec<&Diagnostic> {
    result.diagnostics.iter().filter(|d| d.is_error()).collect()
}

fn said(diagnostic: &Diagnostic) -> String {
    format!(
        "{} | {}",
        diagnostic.message,
        diagnostic.note.as_deref().unwrap_or_default()
    )
}

fn build(main: &str) -> Program {
    let result = compile(main);
    let errors = errors_of(&result);
    assert!(errors.is_empty(), "should compile:\n{main}\n{errors:?}");
    result.program.expect("a program with no errors")
}

fn assert_compiles(main: &str) {
    let result = compile(main);
    assert!(
        result.succeeded(),
        "should compile:\n{main}\n{:?}",
        errors_of(&result)
            .iter()
            .map(|d| said(d))
            .collect::<Vec<_>>()
    );
}

/// The refusals of `main`.
fn refusals(main: &str) -> Vec<Diagnostic> {
    let control = compile("fn void Probe() { }");
    assert!(
        control.succeeded(),
        "the loader compiles a plain module: {:?}",
        errors_of(&control)
    );
    let result = compile(main);
    assert!(
        !result.succeeded(),
        "should be refused, and compiled:\n{main}"
    );
    result
        .diagnostics
        .into_iter()
        .filter(Diagnostic::is_error)
        .collect()
}

/// The refusal of `main` saying `fragment`.
fn refused_saying(main: &str, fragment: &str) -> Diagnostic {
    let found = refusals(main);
    found
        .iter()
        .find(|d| said(d).contains(fragment))
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "expected a refusal saying {fragment:?} for\n{main}\ngot {:?}",
                found.iter().map(said).collect::<Vec<_>>()
            )
        })
}

/// What `diagnostic` underlines in `source`.
fn underlined<'s>(source: &'s str, diagnostic: &Diagnostic) -> &'s str {
    &source[diagnostic.span.start as usize..diagnostic.span.end as usize]
}

/// The byte range of the first `needle` in `source`.
fn range_of(source: &str, needle: &str) -> std::ops::Range<u32> {
    let start = source
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} is in the source"));
    start as u32..(start + needle.len()) as u32
}

/// Whether `diagnostic`'s span lies within `range`.
fn within(diagnostic: &Diagnostic, range: &std::ops::Range<u32>) -> bool {
    diagnostic.span.start >= range.start && diagnostic.span.end <= range.end
}

// ─── Name clashes ───────────────────────────────────────────────────────────

/// **The author sees the clash at the line.** A script `component` named like
/// an engine component is refused with the spec's wording, underlining the
/// declaration's name — whether it is the module compiled or one it imports;
/// a `formerly` name of an engine component counts too. The engine module
/// itself declares those names, and is not refused.
#[test]
fn a_name_clash_is_refused_at_compile_time() {
    let engine = compile_module(
        &EngineLoader::new(&[(
            "main.erg",
            "import \"engine/components.erg\";
             fn void F(Entity e) { e.Set(RigidBody { mass: 2.0 }); }",
        )]),
        "main.erg",
    );
    assert!(
        engine.succeeded(),
        "the engine module may declare the engine's components: {:?}",
        errors_of(&engine)
    );

    let main = "fn void Probe() { }\ncomponent RigidBody { float mass = 1.0; }";
    let refusal = refused_saying(main, "`RigidBody` is an engine component");
    let text = said(&refusal);
    assert!(
        text.contains("Choose another name"),
        "the refusal says what to do; got {text:?}"
    );
    assert!(
        text.contains("import \"engine/components.erg\";"),
        "the refusal names the import that brings the engine's; got {text:?}"
    );
    assert_eq!(
        underlined(main, &refusal),
        "RigidBody",
        "the span is the declaration's name"
    );
    assert_eq!(
        refusal.span.start as usize,
        main.find("RigidBody").expect("declared"),
        "on the declaration"
    );

    let imported = compile_with(
        "import \"gear/body.erg\";\nfn void Probe() { }",
        &[("gear/body.erg", "component Collider { float grip = 1.0; }")],
    );
    assert!(
        imported
            .diagnostics
            .iter()
            .any(|d| d.is_error() && said(d).contains("`Collider` is an engine component")),
        "a clash in an imported module is refused too; got {:?}",
        imported.diagnostics.iter().map(said).collect::<Vec<_>>()
    );

    refused_saying(
        "component OldBody { int n = 0; }",
        "`OldBody` is an engine component",
    );
}

/// A component of a name the engine does not use compiles, and is written
/// like an engine one.
#[test]
fn a_component_of_a_new_name_compiles() {
    assert_compiles(
        "component Shield { int charge = 3; }
         fn void F(Entity e) { e.Set(Shield { charge: 5 }); }",
    );
    let imported = compile_with(
        "import \"gear/shield.erg\";
         fn void F(Entity e) { e.Add(Shield { }); }",
        &[("gear/shield.erg", "component Shield { int charge = 3; }")],
    );
    assert!(
        imported.succeeded(),
        "a component from an imported module compiles: {:?}",
        errors_of(&imported)
    );
}

// ─── [Domain(…)] ────────────────────────────────────────────────────────────

/// **`[Domain(X)]` names one of the six domains**, as the engine spells them.
#[test]
fn every_domain_is_accepted() {
    for domain in ["Spatial", "Render", "Audio", "Physics", "Ui", "Script"] {
        assert_compiles(&format!(
            "[Domain({domain})]
             component Health {{ int current = 100; int max = 100; }}"
        ));
    }
}

/// A misspelt domain is refused at the misspelling, and the refusal lists the
/// domains there are.
#[test]
fn an_unknown_domain_is_refused_at_its_name() {
    let main = "[Domain(Phisics)]\ncomponent Health { int current = 100; }";
    let refusal = refused_saying(main, "Phisics");
    assert_eq!(underlined(main, &refusal), "Phisics");
    let text = said(&refusal);
    for domain in ["Spatial", "Physics", "Script"] {
        assert!(
            text.contains(domain),
            "the refusal names the known domains ({domain}); got {text:?}"
        );
    }
}

/// `[Domain]` takes exactly one identifier: none, two, a number or a string
/// are each refused at the attribute.
#[test]
fn domain_takes_exactly_one_name() {
    for attribute in [
        "[Domain]",
        "[Domain()]",
        "[Domain(Physics, Ui)]",
        "[Domain(3)]",
        "[Domain(\"Physics\")]",
    ] {
        let main = format!("{attribute}\ncomponent Health {{ int current = 100; }}");
        let found = refusals(&main);
        let range = range_of(&main, attribute);
        assert!(
            found
                .iter()
                .any(|d| within(d, &range) && said(d).contains("Domain")),
            "`{attribute}` should be refused at the attribute, naming `Domain`; got {:?}",
            found.iter().map(said).collect::<Vec<_>>()
        );
    }
}

/// A component lives in one domain: a second `[Domain]` is refused, at the
/// second.
#[test]
fn domain_is_given_at_most_once() {
    for second in ["[Domain(Ui)]", "[Domain(Physics)]"] {
        let main = format!("[Domain(Physics)] {second}\ncomponent Health {{ int current = 100; }}");
        let found = refusals(&main);
        let first = range_of(&main, "[Domain(Physics)]");
        let at = main[first.end as usize..]
            .find(second)
            .expect("the second attribute")
            + first.end as usize;
        let range = at as u32..(at + second.len()) as u32;
        assert!(
            found
                .iter()
                .any(|d| within(d, &range) && said(d).contains("Domain")),
            "the second `[Domain]` in `{main}` should be refused at it; got {:?}",
            found.iter().map(said).collect::<Vec<_>>()
        );
    }
}

/// **`[Domain]` is the one attribute a component takes.** The DCC's
/// attributes schedule behaviors and mean nothing on data; `[Domain]` places
/// a component's data and means nothing on a struct or a function.
#[test]
fn only_domain_applies_to_a_component() {
    for attribute in ["[Critical]", "[Budget(0.2ms)]", "[Rate(10)]"] {
        let main = format!("{attribute}\ncomponent Health {{ int current = 100; }}");
        let name = &attribute[1..attribute.find(['(', ']']).expect("an attribute name")];
        let found = refusals(&main);
        let range = range_of(&main, attribute);
        assert!(
            found
                .iter()
                .any(|d| within(d, &range) && said(d).contains(name)),
            "`{attribute}` on a component should be refused at it, naming `{name}`; got {:?}",
            found.iter().map(said).collect::<Vec<_>>()
        );
    }

    assert_compiles("[Domain(Physics)]\ncomponent Health { int current = 100; }");
    for main in [
        "[Domain(Physics)]\nstruct Loot { int value = 0; }",
        "[Domain(Physics)]\nfn void F() { }",
    ] {
        let found = refusals(main);
        let range = range_of(main, "[Domain(Physics)]");
        assert!(
            found.iter().any(|d| within(d, &range)),
            "`[Domain]` off a component is refused at it:\n{main}\ngot {:?}",
            found.iter().map(said).collect::<Vec<_>>()
        );
    }
}

// ─── Defaults ───────────────────────────────────────────────────────────────

/// A default must fit its field, and is underlined where it was written.
#[test]
fn a_component_default_must_fit_its_field() {
    let main = "component Health { int current = \"full\"; }";
    let refusal = refused_saying(main, "expected `int`, found `string`");
    assert_eq!(underlined(main, &refusal), "\"full\"");
}

/// **A default is data**, like a struct's: read wherever a row is built or
/// loaded, never run. An expression that computes is refused.
#[test]
fn a_component_default_must_be_a_constant() {
    for (main, default) in [
        ("component Health { int current = 1 + 2; }", "1 + 2"),
        (
            "fn int Hundred() { return 100; }\ncomponent Health { int current = Hundred(); }",
            "Hundred()",
        ),
    ] {
        let refusal = refused_saying(main, "default must be a constant");
        // The default as written inside the component, not a same-named
        // function declared before it.
        let component = main.find("component").expect("a component");
        let found = range_of(&main[component..], default);
        let at = found.start + component as u32..found.end + component as u32;
        assert!(
            within(&refusal, &at),
            "the refusal underlines `{default}`; got {:?}",
            underlined(main, &refusal)
        );
    }
}

/// **A field without a default takes its type's zero**, so a component of
/// numbers, flags, text, optionals and arrays needs none written.
#[test]
fn a_field_without_a_default_takes_its_zero() {
    assert_compiles(
        "component Tally {
             int n;
             float f;
             bool b;
             string s;
             int? maybe;
             int[] list;
             Vec3? heading;
         }
         fn void F(Entity e) { e.Add(Tally { }); }",
    );
}

/// **A type with no zero needs a default** in a script's component, naming the
/// field: an `Entity` (index 0 is a real entity) or a `Vec3` written without
/// one is refused. The engine's module, whose fields are the engine's own
/// rows, is not held to it.
#[test]
fn a_field_with_no_zero_needs_a_default() {
    let engine = compile_module(
        &EngineLoader::new(&[(
            "main.erg",
            "import \"engine/components.erg\";\nfn void Probe() { }",
        )])
        .with_engine_module("component Link { Entity target; Vec3 offset; }"),
        "main.erg",
    );
    assert!(
        engine.succeeded(),
        "the engine module's fields need no default: {:?}",
        errors_of(&engine)
    );

    for (main, field) in [
        ("component Leash { Entity owner; }", "owner"),
        ("component Mover { Vec3 heading; }", "heading"),
    ] {
        let refusal = refused_saying(main, &format!("`{field}`"));
        let text = said(&refusal);
        assert!(
            text.contains("default"),
            "the refusal says the field needs a default; got {text:?}"
        );
    }
}

impl EngineLoader {
    /// The same loader, its engine module replaced by `source`.
    fn with_engine_module(mut self, source: &str) -> Self {
        self.inner = self.inner.with(ENGINE_COMPONENTS_MODULE, source);
        self
    }
}

// ─── What a copy reads ──────────────────────────────────────────────────────

/// **A script component's layout carries its defaults**: a copy is an
/// ordinary struct object, filled from them where a row lacks a field.
#[test]
fn a_script_component_layout_carries_its_defaults() {
    let program = build(
        "component Health { int current = 100; int max = 100; }
         fn int F(Entity e) { return e.Get(Health)?.max ?? -1; }",
    );

    let (_, layout) = program
        .struct_named("Health")
        .expect("a component declaration yields a struct layout");
    assert_eq!(
        layout.fields,
        vec![
            ("current".to_owned(), "int".to_owned()),
            ("max".to_owned(), "int".to_owned()),
        ]
    );
    assert_eq!(
        layout.defaults,
        vec![
            Some(Owned::Scalar(Value::Int(100))),
            Some(Owned::Scalar(Value::Int(100))),
        ]
    );
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

/// A row of a script component that lacks a field — one laid out before the
/// field was added — reads the declared default for it, not the zero.
#[test]
fn a_copy_missing_a_field_takes_its_declared_default() {
    let program = build(
        "component Health { int current = 100; int max = 100; }
         fn int F(Entity e) {
             if (var h = e.Get(Health)) { return h.current * 1000 + h.max; }
             return -1;
         }",
    );
    let projection = ComponentProjection::new(vec![ProjectedComponent::new(
        ComponentName::new("Health"),
        vec![Box::new(Rows(vec![ScriptValue::Struct(vec![(
            "current".to_owned(),
            ScriptValue::Int(5),
        )])]))],
        HashMap::from([(SUBJECT, (0, 0))]),
        None,
    )]);
    let mut host = Host::new().for_entity(SUBJECT);
    host.components = FrameComponents::new(Arc::new(projection));
    let mut machine =
        Machine::new(&program, "F", &[Value::Entity(SUBJECT)]).expect("the function exists");

    assert_eq!(machine.run(&program, &mut host, u64::MAX), Run::Completed);
    assert_eq!(machine.result(), Value::Int(5100));
}

// ─── What the engine could not store ────────────────────────────────────────

/// **A field named twice is refused at the second.** The engine's layout
/// refuses one (`LayoutError::DuplicateField`): a component that compiled
/// with one would never be registered, and every write to it would miss.
#[test]
fn a_component_field_named_twice_is_refused() {
    let main = "component Tally { int n = 1; int n = 2; }";
    let found = refusals(main);
    assert!(
        found.iter().any(|d| said(d).contains("`n`")),
        "the second `n` should be refused; got {:?}",
        found.iter().map(said).collect::<Vec<_>>()
    );
}

/// **A default the engine refuses as not finite is refused here.** `1e39` is
/// past `f32::MAX` and lexes as infinity; the engine's layout refuses a
/// non-finite default, so the component would compile and never register.
#[test]
fn a_component_default_out_of_float_range_is_refused() {
    for main in [
        "component Far { float reach = 1e39; }",
        "component Far { float reach = -1e39; }",
        "component Far { Duration wait = 1e39s; }",
    ] {
        let found = refusals(main);
        assert!(
            !found.is_empty(),
            "an infinite default should be refused:\n{main}"
        );
    }
}

/// **A component may hold an engine value it starts at.** A `Vec3` has no
/// zero, so a component's `Vec3` field needs a default — and the default a
/// script writes for one is a constructor. Refusing it as "not a constant"
/// leaves no way to declare a non-optional `Vec3`, `Quat` or `Color` field,
/// though the engine stores those inline.
#[test]
fn an_engine_value_field_takes_a_constructor_default() {
    assert_compiles(
        "component Aim {
             Vec3 heading = Vec3(0.0, 0.0, 1.0);
             Color tint = Color(1.0, 1.0, 1.0, 1.0);
         }",
    );
}
