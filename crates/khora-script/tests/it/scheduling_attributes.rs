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

//! The scheduling attributes: `[Critical]`, `[Budget(0.2ms)]`, `[Rate(10hz)]`.
//!
//! `[Critical]` takes no argument; `[Budget(d)]` one duration literal above
//! zero; `[Rate(f)]` one frequency literal above zero — a `hz` literal, which
//! is written nowhere else. Each is given at most once, on a behavior or a
//! system, and a refusal names the form to write. What was declared is stored
//! in the behavior's layout, `schedule`, which the script lane reads: the
//! budget in seconds, the rate in hertz.

use khora_script::vm::{Program, Schedule};
use khora_script::{compile_module, CompileOutcome, Diagnostic, MemoryLoader};

fn compile(source: &str) -> CompileOutcome {
    compile_module(&MemoryLoader::new().with("main.erg", source), "main.erg")
}

fn errors(result: &CompileOutcome) -> Vec<&Diagnostic> {
    result.diagnostics.iter().filter(|d| d.is_error()).collect()
}

/// What a diagnostic tells the author: its line and its note.
fn said(diagnostic: &Diagnostic) -> String {
    format!(
        "{} | {}",
        diagnostic.message,
        diagnostic.note.as_deref().unwrap_or_default()
    )
}

fn build(source: &str) -> Program {
    let result = compile(source);
    let found: Vec<String> = errors(&result).into_iter().map(said).collect();
    assert!(found.is_empty(), "should compile:\n{source}\n{found:?}");
    result.program.expect("a program with no errors")
}

/// What `diagnostic` underlines in `source`.
fn underlined<'s>(source: &'s str, diagnostic: &Diagnostic) -> &'s str {
    source
        .get(diagnostic.span.start as usize..diagnostic.span.end as usize)
        .unwrap_or("<out of the file>")
}

/// Whether `text` holds `fragment`, compared without case; a fragment may
/// list `|`-separated alternatives, one of which must be there.
fn holds(text: &str, fragment: &str) -> bool {
    let text = text.to_lowercase();
    fragment
        .split('|')
        .any(|alternative| text.contains(&alternative.to_lowercase()))
}

/// The refusal of `source` saying every one of `fragments`; panics when it
/// compiles or when no error says them all.
fn refused_saying(source: &str, fragments: &[&str]) -> Diagnostic {
    let result = compile(source);
    assert!(
        !result.succeeded(),
        "should be refused, and compiled:\n{source}"
    );
    errors(&result)
        .into_iter()
        .find(|d| fragments.iter().all(|fragment| holds(&said(d), fragment)))
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "expected a refusal of\n{source}\nsaying {fragments:?}; got {:?}",
                errors(&result)
                    .into_iter()
                    .map(|d| format!("{} @ {:?}", said(d), underlined(source, d)))
                    .collect::<Vec<_>>()
            )
        })
}

/// `<attributes> behavior Guard { … }`, a behavior with a field and an
/// `Update`, so the attributes are all a refusal can be about.
fn guard(attributes: &str) -> String {
    format!(
        "{attributes}
behavior Guard {{
    float speed = 1.0;
    void Update(float dt) {{ speed += dt; }}
}}"
    )
}

/// The schedule `Program::layout` stores for `name`.
fn schedule_of(program: &Program, name: &str) -> Schedule {
    program
        .layout(name)
        .unwrap_or_else(|| panic!("`{name}` has a layout"))
        .schedule
}

fn close(found: Option<f32>, wanted: f32) -> bool {
    found.is_some_and(|found| (found - wanted).abs() <= wanted * 1e-4)
}

// ─── The `hz` literal ───────────────────────────────────────────────────────

/// **`[Rate(10hz)]` compiles**, and so does a fractional frequency: the
/// layout keeps the rate in hertz. A name `hz` is still a name — the suffix
/// is one only when it touches the number.
#[test]
fn a_frequency_literal_is_a_rate_in_hertz() {
    let program = build(
        "[Rate(10hz)]
         behavior Guard {
             float hz = 2.0;
             void Update(float dt) { hz += dt; }
         }
         [Rate(2.5hz)]
         behavior Scout { int seen = 0; }",
    );

    let guard = schedule_of(&program, "Guard");
    assert!(
        close(guard.rate, 10.0),
        "`10hz` is 10 per second: {guard:?}"
    );
    let scout = schedule_of(&program, "Scout");
    assert!(
        close(scout.rate, 2.5),
        "`2.5hz` is 2.5 per second: {scout:?}"
    );
}

/// **A frequency is a rate, written in `[Rate(…)]`**: `10hz` as a local's
/// value, inside an expression, as a field's default or as a function's
/// argument is refused, and the refusal says where a rate is written.
#[test]
fn a_frequency_outside_a_rate_attribute_is_refused() {
    for source in [
        "behavior Guard { void Update(float dt) { float r = 10hz; } }",
        "behavior Guard { float speed = 1.0; void Update(float dt) { speed = speed * 10hz; } }",
        "behavior Guard { float rate = 10hz; }",
        "fn void Tune(float r) { }
         behavior Guard { void Update(float dt) { Tune(10hz); } }",
    ] {
        let refusal = refused_saying(source, &["rate", "[Rate("]);
        assert!(
            underlined(source, &refusal).contains("10hz"),
            "refused at the frequency in\n{source}\ngot {:?} @ {:?}",
            said(&refusal),
            underlined(source, &refusal)
        );
    }
}

/// **A negated frequency in a default is refused as a frequency**, as a bare
/// one is: a behavior field's default, inside an array, or a state's datum.
/// "Not a constant" alone would send the author looking for what to compute;
/// the reason is that a frequency is no value at all.
#[test]
fn a_negated_frequency_in_a_default_is_refused_as_a_rate() {
    for source in [
        "behavior Guard { float rate = -10hz; }",
        "behavior Guard { float[] rates = [-10hz]; }",
        "behavior Guard { state Alert { float rate = -10hz; } }",
    ] {
        let refusal = refused_saying(source, &["rate", "[Rate("]);
        assert!(
            underlined(source, &refusal).contains("10hz"),
            "refused at the frequency in\n{source}\ngot {:?} @ {:?}",
            said(&refusal),
            underlined(source, &refusal)
        );
    }
}

/// **`hz` is a suffix only as a whole word touching the number**: `1e1hz` and
/// `1_0hz` are ten hertz, while `10 hz`, `10hza` and a lone `hz` are a number
/// and a name, as `2 s` is for durations.
#[test]
fn hz_is_a_suffix_only_when_it_touches_the_number_as_a_whole_word() {
    let kinds: Vec<khora_script::TokenKind> = khora_script::lex("1e1hz 1_0hz 10 hz 10hza hz")
        .tokens
        .into_iter()
        .map(|token| token.kind)
        .filter(|kind| *kind != khora_script::TokenKind::Eof)
        .collect();
    use khora_script::TokenKind::{Frequency, Ident, Int};
    assert_eq!(
        kinds,
        vec![
            Frequency(10.0),
            Frequency(10.0),
            Int(10),
            Ident("hz".to_owned()),
            Int(10),
            Ident("hza".to_owned()),
            Ident("hz".to_owned()),
        ]
    );
}

// ─── `[Critical]` ───────────────────────────────────────────────────────────

/// **`[Critical]` stands alone**: with no argument it compiles; given one it
/// is refused, saying it is written `[Critical]`.
#[test]
fn critical_takes_no_argument() {
    build(&guard("[Critical]"));

    for attribute in ["[Critical(1)]", "[Critical(true)]", "[Critical()]"] {
        let source = guard(attribute);
        let refusal = refused_saying(&source, &["`[Critical]`"]);
        assert!(
            refusal.span.end as usize <= attribute.len(),
            "refused at `{attribute}`, got {:?}",
            underlined(&source, &refusal)
        );
    }
}

// ─── `[Budget(…)]` ──────────────────────────────────────────────────────────

/// **A budget is a duration above zero**: `0.2ms` and `1s` compile, and the
/// layout keeps it in seconds.
#[test]
fn budget_takes_one_positive_duration_stored_in_seconds() {
    for (attribute, seconds) in [("[Budget(0.2ms)]", 0.0002), ("[Budget(1s)]", 1.0)] {
        let program = build(&guard(attribute));
        let schedule = schedule_of(&program, "Guard");
        assert!(
            close(schedule.budget, seconds),
            "`{attribute}` is {seconds}s: {schedule:?}"
        );
    }
}

/// **Anything but one duration above zero is refused**, saying the form to
/// write — a duration, as `[Budget(0.2ms)]`: none, a bare number, zero, a
/// negative, a frequency, two durations.
#[test]
fn budget_refuses_anything_but_one_positive_duration() {
    for attribute in [
        "[Budget]",
        "[Budget()]",
        "[Budget(0.5)]",
        "[Budget(1)]",
        "[Budget(0ms)]",
        "[Budget(-1ms)]",
        "[Budget(10hz)]",
        "[Budget(1ms, 2ms)]",
    ] {
        let source = guard(attribute);
        let refusal = refused_saying(&source, &["[Budget(", "ms"]);
        assert!(
            refusal.span.end as usize <= attribute.len(),
            "`{attribute}` refused at itself, got {:?}",
            underlined(&source, &refusal)
        );
    }
}

// ─── `[Rate(…)]` ────────────────────────────────────────────────────────────

/// **A rate is a frequency above zero**: anything else is refused, saying
/// the form to write — a `hz` literal, as `[Rate(10hz)]`: none, a bare
/// number, a duration, zero, a negative, two frequencies.
#[test]
fn rate_refuses_anything_but_one_positive_frequency() {
    for attribute in [
        "[Rate]",
        "[Rate()]",
        "[Rate(10)]",
        "[Rate(10.0)]",
        "[Rate(0.1s)]",
        "[Rate(0hz)]",
        "[Rate(-5hz)]",
        "[Rate(1hz, 2hz)]",
    ] {
        let source = guard(attribute);
        let refusal = refused_saying(&source, &["[Rate(", "hz"]);
        assert!(
            refusal.span.end as usize <= attribute.len(),
            "`{attribute}` refused at itself, got {:?}",
            underlined(&source, &refusal)
        );
    }
}

// ─── Once each, together ────────────────────────────────────────────────────

/// **Each attribute is given once**: a second one is refused at itself,
/// naming the attribute.
#[test]
fn each_scheduling_attribute_is_given_at_most_once() {
    for (first, second, name) in [
        ("[Critical]", "[Critical]", "Critical"),
        ("[Budget(0.2ms)]", "[Budget(1ms)]", "Budget"),
        ("[Rate(10hz)]", "[Rate(5hz)]", "Rate"),
    ] {
        let source = guard(&format!("{first} {second}"));
        let refusal = refused_saying(&source, &[name, "twice|once"]);
        let at = first.len() + 1;
        assert!(
            refusal.span.start as usize >= at && refusal.span.end as usize <= at + second.len(),
            "the second `{name}` is refused, not the first: {:?} @ {:?}",
            said(&refusal),
            underlined(&source, &refusal)
        );
    }
}

/// **The three together compile** on a behavior, beside `[Domain]`, and the
/// layout keeps each as declared.
#[test]
fn critical_budget_and_rate_together_are_stored_as_declared() {
    let program = build(&guard(
        "[Critical] [Budget(0.2ms)] [Rate(10hz)] [Domain(Physics)]",
    ));

    let schedule = schedule_of(&program, "Guard");
    assert!(schedule.critical, "`[Critical]`: {schedule:?}");
    assert!(close(schedule.budget, 0.0002), "`0.2ms`: {schedule:?}");
    assert!(close(schedule.rate, 10.0), "`10hz`: {schedule:?}");
}

// ─── Stored ─────────────────────────────────────────────────────────────────

/// **A behavior declaring nothing has the default schedule** — not critical,
/// no budget, no rate — beside one that declares all three in the same
/// program.
#[test]
fn a_behavior_without_attributes_has_the_default_schedule() {
    let program = build(&format!(
        "{}
         behavior Plain {{ int n = 0; }}",
        guard("[Critical] [Budget(1s)] [Rate(2.5hz)]")
    ));

    assert_eq!(
        schedule_of(&program, "Plain"),
        Schedule {
            critical: false,
            budget: None,
            rate: None,
        }
    );
    let guard = schedule_of(&program, "Guard");
    assert!(
        guard.critical && close(guard.budget, 1.0) && close(guard.rate, 2.5),
        "the declared ones are kept: {guard:?}"
    );
}

/// **A program read back from its serialized form keeps each schedule.**
#[test]
fn a_program_read_back_keeps_its_schedules() {
    let written = build(&format!(
        "{}
         behavior Plain {{ int n = 0; }}",
        guard("[Critical] [Budget(0.2ms)] [Rate(10hz)]")
    ));
    let text = serde_json::to_string(&written).expect("a program serializes");
    let program: Program = serde_json::from_str(&text).expect("and reads back");

    assert_eq!(
        schedule_of(&program, "Guard"),
        schedule_of(&written, "Guard")
    );
    assert!(
        schedule_of(&program, "Guard").critical,
        "the declared schedule, not a default"
    );
    assert_eq!(schedule_of(&program, "Plain"), Schedule::default());
}

/// **A program serialized before schedules were stored reads back** with the
/// default schedule for every behavior.
#[test]
fn a_program_serialized_without_schedules_reads_back_with_the_defaults() {
    let written = build(&guard("[Critical]"));
    let mut frozen = serde_json::to_value(&written).expect("a program serializes");
    let behaviors = frozen
        .get_mut("behaviors")
        .and_then(serde_json::Value::as_array_mut)
        .expect("a program lists its behaviors");
    for behavior in behaviors.iter_mut() {
        behavior
            .as_object_mut()
            .expect("a layout is an object")
            .remove("schedule");
    }

    let program: Program =
        serde_json::from_value(frozen).expect("a layout with no schedule reads back");
    assert_eq!(schedule_of(&program, "Guard"), Schedule::default());
}

// ─── On a system ────────────────────────────────────────────────────────────

/// **A system takes the scheduling attributes too**, held to the same rules
/// and stored the same way.
#[test]
fn a_system_takes_and_stores_the_scheduling_attributes() {
    let program = build(
        "[Critical] [Budget(0.5ms)] [Rate(4hz)]
         system Sweeper {
             int count = 0;
             void Update(float dt) { count += 1; }
         }",
    );

    let schedule = schedule_of(&program, "Sweeper");
    assert!(schedule.critical, "{schedule:?}");
    assert!(close(schedule.budget, 0.0005), "{schedule:?}");
    assert!(close(schedule.rate, 4.0), "{schedule:?}");

    refused_saying(
        "[Rate(4)]\nsystem Sweeper { int count = 0; }",
        &["[Rate(", "hz"],
    );
    refused_saying(
        "[Budget(0.5)]\nsystem Sweeper { int count = 0; }",
        &["[Budget(", "ms"],
    );
}

// ─── Not code ───────────────────────────────────────────────────────────────

/// **Scheduling a behavior changes none of its code**: adding `[Critical]`,
/// `[Budget(…)]` and `[Rate(…)]` leaves every function's fingerprint, and the
/// program's, as they were — so a sequence suspended before the edit resumes
/// exactly after it, instead of being matched site by site.
#[test]
fn adding_scheduling_attributes_leaves_the_program_fingerprint_unchanged() {
    let body = "behavior Guard {
        float speed = 1.0;
        void Update(float dt) { speed += dt; }
        async void Patrol() { await 1s; speed = 0.0; }
    }";
    let plain = build(body);
    let scheduled = build(&format!("[Critical] [Budget(0.2ms)] [Rate(10hz)]\n{body}"));

    let fingerprints = |program: &Program| {
        let mut all: Vec<(String, u64)> = program
            .functions
            .iter()
            .map(|function| (function.name.clone(), function.fingerprint))
            .collect();
        all.sort();
        all
    };
    assert_eq!(fingerprints(&plain), fingerprints(&scheduled));
    assert_eq!(plain.fingerprint(), scheduled.fingerprint());
    assert_ne!(
        schedule_of(&plain, "Guard"),
        schedule_of(&scheduled, "Guard"),
        "the schedules themselves differ"
    );
}
