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

//! A behavior is a component (Ergon spec 09): what a program declares for
//! each `behavior` its project writes — the behavior's own component, its
//! authored fields in the behavior's `[Domain]`, and its **state companion**
//! `Name#state`, the same fixed layout for every behavior, always in the
//! `Script` domain — and the names the compiler refuses a behavior.

use khora_core::ecs::SemanticDomain;
use khora_core::math::Vec3;
use khora_core::script::{ErgonType, ScriptValue};
use khora_script::modules::SourceLoader;
use khora_script::vm::{DeclaredComponent, DeclaredKind, Program};
use khora_script::{compile_module, CompileOutcome, Diagnostic, MemoryLoader};

/// The engine module, as the mirror would serve it.
const ENGINE: &str = "
component RigidBody {
    float mass;
}
";

/// The name of every engine component — what `PreludeLoader` answers,
/// `formerly` names included.
const ENGINE_NAMES: [&str; 2] = ["RigidBody", "OldBody"];

/// The state companion's fields, in order, each with its type's signature —
/// the any-value ones `<any>`, a spelling no struct name can take — the same
/// for every behavior. `fault` is any value: a recorded fault carries the
/// fingerprint of the program it faulted in beside its reason.
const COMPANION: [(&str, &str); 7] = [
    ("state", "string?"),
    ("state_fields", "<any>"),
    ("timers", "<any>"),
    ("pending", "<any>"),
    ("spawned", "bool"),
    ("fault", "<any>"),
    ("resume_failed", "<any>"),
];

/// A loader serving `modules` and the engine module, naming the engine's
/// components the way the engine's own loader does.
struct EngineLoader {
    inner: MemoryLoader,
    names: Vec<String>,
}

impl EngineLoader {
    fn new(modules: &[(&str, &str)]) -> Self {
        let mut inner = MemoryLoader::new().with(khora_script::ENGINE_COMPONENTS_MODULE, ENGINE);
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

fn compile_with(main: &str, others: &[(&str, &str)]) -> CompileOutcome {
    let mut modules = vec![("main.erg", main)];
    modules.extend_from_slice(others);
    compile_module(&EngineLoader::new(&modules), "main.erg")
}

fn said(diagnostic: &Diagnostic) -> String {
    format!(
        "{} | {}",
        diagnostic.message,
        diagnostic.note.as_deref().unwrap_or_default()
    )
}

fn build_with(main: &str, others: &[(&str, &str)]) -> Program {
    let result = compile_with(main, others);
    let errors: Vec<String> = result
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(said)
        .collect();
    assert!(errors.is_empty(), "should compile:\n{main}\n{errors:?}");
    result.program.expect("a program with no errors")
}

fn build(main: &str) -> Program {
    build_with(main, &[])
}

/// The errors of `main` compiled beside `others`; panics when it compiles.
fn refusals_with(main: &str, others: &[(&str, &str)]) -> Vec<Diagnostic> {
    let result = compile_with(main, others);
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

/// What `diagnostic` underlines in `source`.
fn underlined<'s>(source: &'s str, diagnostic: &Diagnostic) -> &'s str {
    &source[diagnostic.span.start as usize..diagnostic.span.end as usize]
}

/// The byte range of the first `needle` in `source` at or after `from`.
fn range_after(source: &str, needle: &str, from: usize) -> std::ops::Range<u32> {
    let start = source[from..]
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} is in the source"))
        + from;
    start as u32..(start + needle.len()) as u32
}

fn within(diagnostic: &Diagnostic, range: &std::ops::Range<u32>) -> bool {
    diagnostic.span.start >= range.start && diagnostic.span.end <= range.end
}

fn declared<'p>(program: &'p Program, name: &str) -> &'p DeclaredComponent {
    program
        .declared
        .iter()
        .find(|declared| declared.name == name)
        .unwrap_or_else(|| {
            panic!(
                "`{name}` is declared; the program declares {:?}",
                program
                    .declared
                    .iter()
                    .map(|declared| declared.name.as_str())
                    .collect::<Vec<_>>()
            )
        })
}

/// Each field's name and its type as written.
fn shape(declared: &DeclaredComponent) -> Vec<(String, String)> {
    declared
        .fields
        .iter()
        .map(|field| (field.name.clone(), field.ty.signature()))
        .collect()
}

fn companion_shape() -> Vec<(String, String)> {
    COMPANION
        .iter()
        .map(|(name, ty)| ((*name).to_owned(), (*ty).to_owned()))
        .collect()
}

// ─── The two declarations ───────────────────────────────────────────────────

/// **A behavior declares two components**: `Guard`, its fields in declaration
/// order with their types and registered defaults, in the `Script` domain
/// when no `[Domain]` is written; and `Guard#state`, its state companion —
/// each naming the module that wrote the behavior, sorted by name.
#[test]
fn a_behavior_declares_its_component_and_its_state_companion() {
    let program = build(
        "behavior Guard {
             float speed = 1.5;
             int hp;
             string? title;
             void Update(float dt) { hp += 1; }
         }",
    );

    assert_eq!(
        program
            .declared
            .iter()
            .map(|declared| declared.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Guard", "Guard#state"],
        "a behavior declares its component and its companion, sorted by name"
    );

    let guard = declared(&program, "Guard");
    assert_eq!(guard.kind, DeclaredKind::Behavior);
    assert_eq!(guard.module, "main.erg");
    assert_eq!(guard.domain, SemanticDomain::Script);
    assert_eq!(
        guard
            .fields
            .iter()
            .map(|field| (field.name.as_str(), field.ty.clone(), field.default.clone()))
            .collect::<Vec<_>>(),
        vec![
            ("speed", ErgonType::Float, ScriptValue::Float(1.5)),
            ("hp", ErgonType::Int, ScriptValue::Int(0)),
            (
                "title",
                ErgonType::Optional(Box::new(ErgonType::Str)),
                ScriptValue::Null
            ),
        ]
    );

    let state = declared(&program, "Guard#state");
    assert_eq!(
        state.kind,
        DeclaredKind::BehaviorState {
            behavior: "Guard".to_owned()
        }
    );
    assert_eq!(state.module, "main.erg");
    assert_eq!(state.domain, SemanticDomain::Script);
    assert_eq!(shape(state), companion_shape());
}

/// **Every state companion has one layout**, whatever its behavior holds —
/// fields, states, timers, an `await` — and starts with no state, nothing
/// spawned, and no fault.
#[test]
fn every_state_companion_has_the_same_fixed_layout() {
    let program = build(
        "behavior Plain { }
         behavior Busy {
             int count = 0;
             Vec3 home = Vec3(0.0, 0.0, 0.0);
             every 0.5s { count += 1; }
             state Idle { int waited = 0; }
             state Alert { float since = 0.0; }
             async void Strike() { await 0.5s; count += 2; }
         }",
    );

    let plain = declared(&program, "Plain#state");
    let busy = declared(&program, "Busy#state");
    assert_eq!(shape(plain), companion_shape());
    assert_eq!(
        busy.fields, plain.fields,
        "the companion's layout is the same for every behavior"
    );
    let default = |name: &str| {
        plain
            .fields
            .iter()
            .find(|field| field.name == name)
            .map(|field| field.default.clone())
            .unwrap_or_else(|| panic!("the companion has `{name}`"))
    };
    assert_eq!(default("state"), ScriptValue::Null);
    assert_eq!(default("spawned"), ScriptValue::Bool(false));
    assert_eq!(default("fault"), ScriptValue::Null);
    for field in &plain.fields {
        assert!(
            field.ty.admits(&field.default),
            "`{}`'s default {:?} is a value of its type `{}`",
            field.name,
            field.default,
            field.ty
        );
    }
}

/// **`[Domain(X)]` places a behavior's fields**, beside the DCC's attributes;
/// its companion stays in `Script` whatever the behavior's domain.
#[test]
fn a_behavior_takes_its_domain_beside_the_scheduling_attributes() {
    for domain in SemanticDomain::ALL.iter().copied() {
        let program = build(&format!(
            "[Critical] [Domain({})] [Budget(0.2ms)] [Rate(10hz)]
             behavior Hover {{ float height = 0.4; }}",
            domain.name()
        ));
        assert_eq!(
            declared(&program, "Hover").domain,
            domain,
            "`[Domain({})]`",
            domain.name()
        );
        assert_eq!(
            declared(&program, "Hover#state").domain,
            SemanticDomain::Script,
            "the companion of a `[Domain({})]` behavior",
            domain.name()
        );
    }
}

/// **A field's registered default** is its constant default (an `int`
/// written for a `float` is that float, a duration its seconds), else its
/// type's zero, else — for a type with no zero — its neutral value.
#[test]
fn a_behavior_field_registers_its_constant_default_else_its_zero_else_its_neutral_value() {
    let program = build(
        "behavior Guard {
             float speed = 2;
             Duration cooldown = 2s;
             int count;
             string[] names;
             Vec3 home = Vec3(1.0, 2.0, 3.0);
             Vec3 heading;
         }",
    );
    let guard = declared(&program, "Guard");
    let field = |name: &str| {
        guard
            .fields
            .iter()
            .find(|field| field.name == name)
            .unwrap_or_else(|| panic!("`Guard` declares `{name}`"))
    };

    assert_eq!(field("speed").ty, ErgonType::Float);
    assert_eq!(field("speed").default, ScriptValue::Float(2.0));
    assert_eq!(field("cooldown").ty, ErgonType::Duration);
    assert_eq!(field("cooldown").default, ScriptValue::Float(2.0));
    assert_eq!(field("count").ty, ErgonType::Int);
    assert_eq!(field("count").default, ScriptValue::Int(0));
    assert_eq!(field("names").default, ScriptValue::Array(Vec::new()));
    for (name, wanted) in [
        ("home", Vec3::new(1.0, 2.0, 3.0)),
        ("heading", Vec3::new(0.0, 0.0, 0.0)),
    ] {
        assert_eq!(field(name).ty, ErgonType::Engine("Vec3"));
        assert_eq!(field(name).default, ScriptValue::Vec3(wanted), "`{name}`");
    }
    assert_eq!(
        guard
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        vec!["speed", "cooldown", "count", "names", "home", "heading"],
        "in declaration order"
    );
}

/// **A component stays a component** beside a behavior, and a behavior in an
/// imported module names that module.
#[test]
fn a_component_declared_beside_a_behavior_stays_a_component() {
    let program = build_with(
        "import \"ai/patrol.erg\";
         component Loot { int value = 1; }
         behavior Guard { int hp = 3; }",
        &[("ai/patrol.erg", "behavior Patrol { float pace = 1.0; }")],
    );

    assert_eq!(
        program
            .declared
            .iter()
            .map(|declared| (
                declared.name.as_str(),
                declared.module.as_str(),
                declared.kind.clone()
            ))
            .collect::<Vec<_>>(),
        vec![
            ("Guard", "main.erg", DeclaredKind::Behavior),
            (
                "Guard#state",
                "main.erg",
                DeclaredKind::BehaviorState {
                    behavior: "Guard".to_owned()
                }
            ),
            ("Loot", "main.erg", DeclaredKind::Component),
            ("Patrol", "ai/patrol.erg", DeclaredKind::Behavior),
            (
                "Patrol#state",
                "ai/patrol.erg",
                DeclaredKind::BehaviorState {
                    behavior: "Patrol".to_owned()
                }
            ),
        ]
    );
}

/// **A program frozen before behaviors were components still reads**: a
/// declaration recorded with no kind is a `component`'s.
#[test]
fn a_declaration_frozen_without_a_kind_reads_as_a_component() {
    let program = build("component Loot { int value = 1; }");
    let mut frozen = serde_json::to_value(&program.declared[0]).expect("a declaration serialises");
    let removed = frozen
        .as_object_mut()
        .expect("a declaration is a JSON object")
        .remove("kind");
    assert!(removed.is_some(), "the declaration records its kind");

    let read: DeclaredComponent =
        serde_json::from_value(frozen).expect("a declaration frozen with no kind reads");
    assert_eq!(read.kind, DeclaredKind::Component);
    assert_eq!(read.name, "Loot");

    let behavior = build("behavior Guard { }");
    for declared in &behavior.declared {
        let json = serde_json::to_string(declared).expect("serialises");
        let back: DeclaredComponent = serde_json::from_str(&json).expect("reads back");
        assert_eq!(&back, declared, "`{}` round-trips", declared.name);
    }
}

// ─── Names ──────────────────────────────────────────────────────────────────

/// **A behavior named like an engine component is refused at its name** —
/// a `formerly` name too, and in an imported module: a behavior is a
/// component, and a component's name is unique among every component.
#[test]
fn a_behavior_named_like_an_engine_component_is_refused_at_its_name() {
    for name in ["RigidBody", "OldBody"] {
        let main = format!("fn void Probe() {{ }}\nbehavior {name} {{ int hp = 1; }}");
        let found = refusals_with(&main, &[]);
        let at = range_after(&main, name, main.find("behavior").expect("a behavior"));
        let refusal = found.iter().find(|d| within(d, &at)).unwrap_or_else(|| {
            panic!(
                "`behavior {name}` should be refused at its name; got {:?}",
                found.iter().map(said).collect::<Vec<_>>()
            )
        });
        assert_eq!(underlined(&main, refusal), name);
        assert!(
            said(refusal).contains(&format!("`{name}`")),
            "the refusal names `{name}`; got {:?}",
            said(refusal)
        );
    }

    let imported = compile_with(
        "import \"ai/body.erg\";\nfn void Probe() { }",
        &[("ai/body.erg", "behavior RigidBody { }")],
    );
    assert!(
        imported
            .diagnostics
            .iter()
            .any(|d| d.is_error() && said(d).contains("`RigidBody`")),
        "a behavior in an imported module is refused too; got {:?}",
        imported.diagnostics.iter().map(said).collect::<Vec<_>>()
    );
}

/// **A behavior named like a project component, or like another module's
/// behavior, is refused at its name** — in one module, or across two of one
/// program.
#[test]
fn a_behavior_named_like_a_project_component_or_another_behavior_is_refused() {
    let cases: [(&str, &[(&str, &str)]); 3] = [
        (
            "component Guard { int hp = 1; }\nbehavior Guard { int hp = 1; }",
            &[],
        ),
        (
            "import \"loot.erg\";\nbehavior Guard { int hp = 1; }",
            &[("loot.erg", "component Guard { int hp = 1; }")],
        ),
        (
            "import \"ai/guard.erg\";\nbehavior Guard { int hp = 1; }",
            &[("ai/guard.erg", "behavior Guard { float pace = 1.0; }")],
        ),
    ];
    for (main, others) in cases {
        let found = refusals_with(main, others);
        let at = range_after(main, "Guard", main.find("behavior").expect("a behavior"));
        assert!(
            found.iter().any(|d| within(d, &at) && said(d).contains("Guard")),
            "`behavior Guard` should be refused at its name in:\n{main}\nbeside {others:?}\ngot {:?}",
            found.iter().map(said).collect::<Vec<_>>()
        );
    }
}

/// **`[Domain]` on a behavior is held to a component's rules**: a misspelt
/// domain is refused at the misspelling, a second `[Domain]` at the second.
#[test]
fn a_behavior_domain_is_checked_like_a_component_domain() {
    let main = "[Domain(Phisics)]\nbehavior Hover { float height = 0.4; }";
    let found = refusals_with(main, &[]);
    assert!(
        found.iter().any(|d| underlined(main, d) == "Phisics"),
        "the misspelt domain should be refused at it; got {:?}",
        found.iter().map(said).collect::<Vec<_>>()
    );

    let main = "[Domain(Physics)] [Domain(Ui)]\nbehavior Hover { float height = 0.4; }";
    let found = refusals_with(main, &[]);
    let second = range_after(main, "[Domain(Ui)]", 0);
    assert!(
        found
            .iter()
            .any(|d| within(d, &second) && said(d).contains("Domain")),
        "the second `[Domain]` should be refused at it; got {:?}",
        found.iter().map(said).collect::<Vec<_>>()
    );
}

/// **A state companion is the engine's to write**: the program declares it,
/// but no script can name it — not to write it, remove it, or read it.
#[test]
fn a_state_companion_is_declared_where_no_script_can_name_it() {
    let program = build("behavior Guard { int hp = 1; }");
    assert_eq!(
        declared(&program, "Guard#state").kind,
        DeclaredKind::BehaviorState {
            behavior: "Guard".to_owned()
        }
    );

    for body in [
        "e.Set(Guard#state { spawned: true });",
        "e.Add(Guard#state { });",
        "e.Remove(Guard#state);",
        "bool held = e.Has(Guard#state);",
    ] {
        let main = format!("behavior Guard {{ int hp = 1; }}\nfn void F(Entity e) {{ {body} }}");
        let result = compile_with(&main, &[]);
        assert!(
            !result.succeeded(),
            "a script cannot name `Guard#state`:\n{main}"
        );
    }
}

// ─── Field types and defaults ───────────────────────────────────────────────

/// **A behavior and a component declaring one field alike register one
/// default**: a behavior field's constant default is the value a component
/// field of the same declaration starts at — an angle in the unit both store,
/// a duration in seconds, an `int` written inside a struct or an array where
/// a `float` is declared.
#[test]
fn a_behavior_field_registers_the_default_a_component_field_does() {
    let fields = "Angle turn = 90deg;
             Duration pause = 250ms;
             float speed = -2;
             float[] steps = [1, 2.5];
             float? drift = 3;
             Inner inner = Inner { x: 1 };
             Outer outer = Outer { n: 2 };
             Color tint = Color(1.0, 0.5, 0.25, 1.0);";
    let program = build(&format!(
        "struct Inner {{ float x = 0.0; }}
         struct Outer {{ Inner inner = Inner {{ x: 4.0 }}; int n = 0; }}
         component Tuned {{ {fields} }}
         behavior Guard {{ {fields} void Update(float dt) {{ }} }}"
    ));
    let component = declared(&program, "Tuned");
    let behavior = declared(&program, "Guard");
    for (theirs, ours) in component.fields.iter().zip(&behavior.fields) {
        assert_eq!(theirs.name, ours.name);
        assert_eq!(
            ours.ty, theirs.ty,
            "`{}`: a behavior field is typed as a component field",
            ours.name
        );
        assert_eq!(
            ours.default, theirs.default,
            "`{}`: a behavior field registers the default a component field does",
            ours.name
        );
    }
}
