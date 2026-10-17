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

//! The inspector attributes of a field: `[Range(min, max)]`,
//! `[Tooltip("…")]`, `[Header("…")]`.
//!
//! They are written before a field of a `component`, a `behavior` or a
//! `system` — the fields the inspector shows. A range takes two bounds of the
//! field's own kind (integers on an `int`, numbers on a `float`, durations on
//! a `Duration`, angles on an `Angle`, the element's kind on an optional or an
//! array of one), not empty, holding the field's constant default. A tooltip
//! and a header take one non-blank text each. Each is given at most once; a
//! struct's field and a state's field take none. A refusal sits on the
//! attribute and names what to write.
//!
//! What was declared travels as the field's `hints` in the program's
//! declarations, the range in the field's stored unit — seconds, radians —
//! and nothing the program runs changes with it.

mod carried;
mod parse_recovery;
mod placement;
mod range_defaults;
mod refusals;
mod texts;

use khora_core::script::FieldHints;
use khora_script::vm::{DeclaredComponent, DeclaredField, Program};
use khora_script::{compile_module, CompileOutcome, Diagnostic, MemoryLoader};

/// A struct every host declares, so a field may hold one.
const LOOT: &str = "struct Loot { int value = 1; }";

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

/// The refusal of `source` saying every one of `fragments`, asserted to sit
/// on the first `attribute` written in it.
fn refused_at(source: &str, attribute: &str, fragments: &[&str]) -> Diagnostic {
    let refusal = refused_saying(source, fragments);
    let start = source
        .find(attribute)
        .unwrap_or_else(|| panic!("{attribute:?} is written in\n{source}"));
    let end = start + attribute.len();
    assert!(
        refusal.span.start as usize >= start
            && refusal.span.end as usize <= end
            && refusal.span.start < refusal.span.end,
        "refused at `{attribute}` in\n{source}\ngot {:?} @ {:?}",
        said(&refusal),
        underlined(source, &refusal)
    );
    refusal
}

/// `field` written on a component, beside a field without attributes.
fn on_component(field: &str) -> String {
    format!(
        "{LOOT}
component Stats {{
    {field}
    int other = 0;
}}"
    )
}

/// `field` written on a behavior, beside a field without attributes and an
/// `Update`.
fn on_behavior(field: &str) -> String {
    format!(
        "{LOOT}
behavior Guard {{
    {field}
    int other = 0;
    void Update(float dt) {{ other += 1; }}
}}"
    )
}

/// `field` written on a system, beside a field without attributes and an
/// `Update`.
fn on_system(field: &str) -> String {
    format!(
        "{LOOT}
system Sweeper {{
    {field}
    int other = 0;
    void Update(float dt) {{ other += 1; }}
}}"
    )
}

/// What an item is, how to write a field on it, and the name of the
/// component it declares.
type Host = (&'static str, fn(&str) -> String, &'static str);

/// Every item whose fields the inspector shows.
const HOSTS: [Host; 3] = [
    ("component", on_component, "Stats"),
    ("behavior", on_behavior, "Guard"),
    ("system", on_system, "Sweeper"),
];

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

fn field<'d>(declared: &'d DeclaredComponent, name: &str) -> &'d DeclaredField {
    declared
        .fields
        .iter()
        .find(|field| field.name == name)
        .unwrap_or_else(|| panic!("`{}` declares `{name}`", declared.name))
}

/// The hints the component `component` carries for its field `name`.
fn hints<'p>(program: &'p Program, component: &str, name: &str) -> &'p FieldHints {
    &field(declared(program, component), name).hints
}
