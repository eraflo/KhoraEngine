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

//! A `system` is a behavior with no subject.
//!
//! What the compiler accepts and refuses about one: fields, `Update`,
//! `every`/`after` and methods compile; whatever needs the entity a behavior
//! is attached to — `this`, `Position()` — or belongs to one entity's time —
//! a `state`, `become`, an `on` handler, `async` and `await` — is refused at
//! the construct, saying a system has none. A system's name lives in the one
//! namespace components, behaviors and structs share, and takes the
//! scheduling attributes but no `[Domain]`. Inside it a query may `ref` a
//! component, never a behavior's.

use khora_script::{
    compile_module, CompileOutcome, Diagnostic, MemoryLoader, ENGINE_COMPONENTS_MODULE,
};

/// The engine module, as the mirror would serve it.
const ENGINE: &str = "
component Health { int current; int max; }
component Regen { float rate; }
component Player { int team; }
component Dead { float since; }
";

/// The system of the plan, members of every kind a system takes.
const REGENERATION: &str = "system Regeneration {
    float total = 0.0;
    int sweeps = 0;
    void Update(float dt) {
        foreach (Entity e, ref Health h, Regen r) with Player without Dead {
            h.current = h.current + 1;
            total += r.rate * dt;
        }
        sweeps += Helper();
    }
    every 2s {
        total = 0.0;
    }
    after 10s {
        sweeps = 0;
    }
    int Helper() { return 1; }
}";

fn source_of(main: &str) -> String {
    format!("import \"{ENGINE_COMPONENTS_MODULE}\";\n{main}")
}

fn compile(main: &str) -> CompileOutcome {
    let loader = MemoryLoader::new()
        .with(ENGINE_COMPONENTS_MODULE, ENGINE)
        .with("main.erg", source_of(main));
    compile_module(&loader, "main.erg")
}

fn said(diagnostic: &Diagnostic) -> String {
    format!(
        "{} | {}",
        diagnostic.message,
        diagnostic.note.as_deref().unwrap_or_default()
    )
}

fn errors_of(result: &CompileOutcome) -> Vec<&Diagnostic> {
    result.diagnostics.iter().filter(|d| d.is_error()).collect()
}

fn compiles(main: &str) {
    let result = compile(main);
    let errors: Vec<String> = errors_of(&result).into_iter().map(said).collect();
    assert!(errors.is_empty(), "should compile:\n{main}\n{errors:?}");
    assert!(result.program.is_some(), "a program with no errors");
}

/// What `diagnostic` underlines in `main` (as compiled, after its import).
fn underlined(main: &str, diagnostic: &Diagnostic) -> String {
    source_of(main)
        .get(diagnostic.span.start as usize..diagnostic.span.end as usize)
        .unwrap_or("<out of the file>")
        .to_owned()
}

/// The refusal of `main` saying every one of `fragments` and underlining
/// `snippet` (or one of its `|`-separated alternatives) — after proving the plan's system compiles, so the refusal is
/// about what `main` adds to it.
fn refused(main: &str, fragments: &[&str], snippet: &str) -> Diagnostic {
    compiles(REGENERATION);
    let result = compile(main);
    assert!(
        !result.succeeded(),
        "should be refused, and compiled:\n{main}"
    );
    errors_of(&result)
        .into_iter()
        .find(|d| {
            let text = said(d);
            fragments.iter().all(|fragment| text.contains(fragment))
                && snippet
                    .split('|')
                    .any(|alternative| underlined(main, d).contains(alternative))
        })
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "expected a refusal saying {fragments:?} at `{snippet}`; got {:?}",
                errors_of(&result)
                    .into_iter()
                    .map(|d| format!("{} @ {:?}", said(d), underlined(main, d)))
                    .collect::<Vec<_>>()
            )
        })
}

/// `system Sweeper { int count = 0; <members> }`.
fn sweeper(members: &str) -> String {
    format!("system Sweeper {{ int count = 0; {members} }}")
}

// ─── What a system takes ────────────────────────────────────────────────────

/// **Fields, `Update`, `every`, `after` and methods** compile in a system —
/// and a query in it may `ref` a component and write its fields.
#[test]
fn a_system_with_fields_update_timers_and_methods_compiles() {
    compiles(REGENERATION);
    compiles("system Empty { }");
    compiles(
        "system Spawner {
             every 2s { }
         }",
    );
}

/// A system beside the other declarations of a file, and calling a free
/// function.
#[test]
fn a_system_sits_beside_behaviors_structs_and_functions() {
    compiles(
        "struct Loot { int value = 0; }
         fn int Twice(int n) { return n * 2; }
         behavior Guard { int hp = 1; }
         system Regeneration {
             int seen = 0;
             void Update(float dt) {
                 foreach (Health h) { seen += Twice(h.current); }
             }
         }",
    );
}

// ─── What a system refuses ──────────────────────────────────────────────────

/// **A system has no `this`**: refused where it is written, in a method, in
/// `Update` and in a timer.
#[test]
fn this_is_refused_in_a_system() {
    for member in [
        "void Update(float dt) { this.Despawn(); }",
        "every 1s { var me = this; }",
        "int Probe() { if (this.Has(Health)) { return 1; } return 0; }",
    ] {
        refused(&sweeper(member), &["`this`", "system"], "this");
    }
}

/// **A native needing a subject is refused**: `Position()` reads the entity
/// a behavior is attached to, and a system has none.
#[test]
fn a_native_needing_a_subject_is_refused_in_a_system() {
    refused(
        &sweeper("void Update(float dt) { var p = Position(); }"),
        &["Position", "system"],
        "Position()",
    );
}

/// **A system has no states**: a `state` member is refused at itself.
#[test]
fn a_state_is_refused_in_a_system() {
    refused(
        &sweeper("state Idle { void Update(float dt) { count += 1; } }"),
        &["state", "system"],
        "Idle",
    );
}

/// **`become` has nowhere to go** in a system.
#[test]
fn become_is_refused_in_a_system() {
    refused(
        &sweeper("void Update(float dt) { become Idle; }"),
        &["become", "system"],
        "become",
    );
}

/// **An event is aimed at an entity**: an `on` handler is refused in a system.
#[test]
fn an_event_handler_is_refused_in_a_system() {
    refused(
        &sweeper("on Hit(int amount) { count += amount; }"),
        &["system"],
        "Hit",
    );
}

/// **A system's time is `every` and `after`**: an `async` member is refused
/// at itself.
#[test]
fn an_async_member_is_refused_in_a_system() {
    refused(
        &sweeper("async void Wait() { count += 1; }"),
        &["async", "system"],
        "async|Wait",
    );
}

/// `await` is refused in a system's members, whatever they are.
#[test]
fn await_is_refused_in_a_system() {
    for member in [
        "void Update(float dt) { await 1s; }",
        "every 2s { await 1s; }",
        "void Later() { await 1s; }",
    ] {
        refused(&sweeper(member), &["await", "system"], "await");
    }
}

/// **One namespace**: a system named like a struct, a behavior, a function
/// or a component — of its own file or of the engine's — is a clash.
#[test]
fn a_system_shares_one_namespace_with_every_declaration() {
    for (main, name) in [
        ("struct Loot { int value = 0; }\nsystem Loot { }", "Loot"),
        ("behavior Guard { int hp = 1; }\nsystem Guard { }", "Guard"),
        ("system Guard { }\nbehavior Guard { int hp = 1; }", "Guard"),
        ("system Twin { }\nsystem Twin { }", "Twin"),
        ("fn void Tick() { }\nsystem Tick { }", "Tick"),
    ] {
        let refusal = refused(main, &[&format!("`{name}`")], name);
        assert!(
            ["twice", "both", "already"]
                .iter()
                .any(|word| said(&refusal).contains(word)),
            "a clash, said as one: {}",
            said(&refusal)
        );
    }

    // `Health` is the engine's: the clash is across files.
    let result = compile("system Health { }");
    assert!(!result.succeeded(), "a system named like a component");
    assert!(
        errors_of(&result)
            .iter()
            .any(|d| said(d).contains("Health")),
        "the clash names `Health`: {:?}",
        errors_of(&result).into_iter().map(said).collect::<Vec<_>>()
    );
}

/// **A system takes the scheduling attributes** — `[Critical]`,
/// `[Budget(…)]`, `[Rate(…)]` — and they compile, alone or together.
#[test]
fn a_system_takes_the_scheduling_attributes() {
    for attribute in [
        "[Critical]",
        "[Budget(0.5ms)]",
        "[Rate(10hz)]",
        "[Critical] [Budget(0.5ms)] [Rate(10hz)]",
    ] {
        compiles(&format!("{attribute}\nsystem Sweeper {{ int count = 0; }}"));
    }
}

/// **A system's fields live in `Script`, and it takes no other attribute**:
/// `[Domain(…)]` and an attribute nobody defines are refused at themselves,
/// and the system after them is parsed.
#[test]
fn domain_and_unknown_attributes_on_a_system_are_refused() {
    for attribute in ["[Domain(Physics)]", "[Fast]"] {
        let main = format!("{attribute}\nsystem Sweeper {{ int count = 0; }}");
        compiles("system Sweeper { int count = 0; }");
        let result = compile(&main);
        assert!(
            !result.succeeded(),
            "an attribute on a system is refused:\n{main}"
        );
        let errors = errors_of(&result);
        assert!(
            errors
                .iter()
                .any(|d| said(d).contains("attribute") && underlined(&main, d).contains(attribute)),
            "refused at `{attribute}`: {:?}",
            errors.iter().map(|d| said(d)).collect::<Vec<_>>()
        );
        assert!(
            errors
                .iter()
                .all(|d| underlined(&main, d).contains(attribute)),
            "the attribute is the only thing refused — the system parses: {:?}",
            errors
                .iter()
                .map(|d| format!("{} @ {:?}", said(d), underlined(&main, d)))
                .collect::<Vec<_>>()
        );
    }
}

// ─── A system's queries ─────────────────────────────────────────────────────

/// **A plain binding is read-only in a system too**: only a `ref` binding's
/// fields are written in place.
#[test]
fn a_plain_binding_is_read_only_in_a_system() {
    compiles(&sweeper(
        "void Update(float dt) { foreach (ref Health h, Regen r) { h.current = 1; } }",
    ));
    refused(
        &sweeper("void Update(float dt) { foreach (ref Health h, Regen r) { r.rate = 2.0; } }"),
        &["`r`", "read-only"],
        "r.rate",
    );
}

/// **A `ref` binding is the component's, not a variable**: assigning the
/// binding itself is refused.
#[test]
fn a_ref_binding_cannot_be_reassigned() {
    refused(
        &sweeper(
            "void Update(float dt) {
                 foreach (ref Health h) { foreach (Health k) { h = k; } }
             }",
        ),
        &["`h`"],
        "h = k",
    );
}

/// **A behavior's fields are its own turn's or `Set`'s**: a system's query may
/// read a behavior's component, never `ref` it.
#[test]
fn ref_on_a_behavior_component_is_refused_in_a_system() {
    compiles(
        "behavior Guard { float health = 10; }
         system Watch { float seen = 0; void Update(float dt) { foreach (Guard g) { seen += g.health; } } }",
    );
    refused(
        "behavior Guard { float health = 10; }
         system Watch { void Update(float dt) { foreach (ref Guard g) { g.health = 1; } } }",
        &["`Guard`"],
        "ref Guard",
    );
}
