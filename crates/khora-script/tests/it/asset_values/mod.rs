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

//! Asset references: `Prefab? bullet;`.
//!
//! Six kinds, each a type of its own — `Prefab`, `Sound`, `Material`, `Mesh`,
//! `Texture`, `Scene` — recognised before a struct of that name could be,
//! wherever a type is written. The value is the asset's identity, its
//! `AssetUUID`; the kind is static, the type's alone.
//!
//! There is no asset literal, so an asset field is always optional and starts
//! at `null`: the inspector chooses the asset. Two assets of one kind compare
//! with `==` and `!=`, and either with `null`; nothing else applies to them.
//! In `$"…"` an asset shows `Asset#` and the first eight hex digits of its
//! UUID.

mod carried;
mod declarations;
mod display;
mod fingerprints;
mod operators;

use khora_core::asset::AssetUUID;
use khora_core::script::ScriptValue;
use khora_script::vm::{Machine, Program, Run, Value};
use khora_script::{compile_module, Arena, CompileOutcome, Diagnostic, Host, MemoryLoader};

/// The six spellings, each with the kind its fields declare.
const KINDS: [&str; 6] = ["Prefab", "Sound", "Material", "Mesh", "Texture", "Scene"];

fn compile(source: &str) -> CompileOutcome {
    compile_module(&MemoryLoader::new().with("main.erg", source), "main.erg")
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

fn listed(source: &str, result: &CompileOutcome) -> Vec<String> {
    errors(result)
        .into_iter()
        .map(|d| format!("{} @ {:?}", said(d), underlined(source, d)))
        .collect()
}

/// Compiles `source`, which must compile without an error.
fn build(source: &str) -> Program {
    let result = compile(source);
    let found = listed(source, &result);
    assert!(found.is_empty(), "should compile:\n{source}\n{found:?}");
    result.program.expect("a program with no errors")
}

/// The refusal of `source` underlining (within) `place`, saying every
/// fragment of `fragments` — each a `|`-separated set of alternatives.
fn refused_at(source: &str, place: &str, fragments: &[&str]) -> Diagnostic {
    let result = compile(source);
    assert!(
        !result.succeeded(),
        "should be refused, and compiled:\n{source}"
    );
    let start = source
        .find(place)
        .unwrap_or_else(|| panic!("{place:?} is written in\n{source}"));
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
                "expected a refusal of\n{source}\nsaying {fragments:?} at `{place}`; got {:?}",
                listed(source, &result)
            )
        })
}

/// An asset identity whose hyphenated form starts `1a2b3c4d`.
fn bullet_uuid() -> AssetUUID {
    AssetUUID::from_bytes([
        0x1a, 0x2b, 0x3c, 0x4d, 0x5e, 0x6f, 0x40, 0x81, 0x92, 0xa3, 0xb4, 0xc5, 0xd6, 0xe7, 0xf8,
        0x09,
    ])
}

/// Another asset identity.
fn other_uuid() -> AssetUUID {
    AssetUUID::from_bytes([0xee; 16])
}

/// `uuid` as a register holds it, through the engine's bridge — the way a
/// field the inspector set reaches a script.
fn asset(uuid: AssetUUID) -> Value {
    khora_script::bridge::to_register(&ScriptValue::Asset(uuid), &mut Arena::new())
        .unwrap_or_else(|e| panic!("an asset reference reaches a register: {e:?}"))
}

/// `value`, a register holding no text, as the engine reads it back.
fn boundary(value: Value) -> ScriptValue {
    khora_script::bridge::from_register(value, &[], &Arena::new())
        .unwrap_or_else(|e| panic!("{value:?} reaches the engine: {e:?}"))
}

fn finish(mut machine: Machine, program: &Program, host: &mut Host) -> Value {
    for _ in 0..1000 {
        match machine.run(program, host, u64::MAX) {
            Run::Completed => return machine.result(),
            Run::Suspended(_) => host.awaiting = None,
            Run::Faulted(fault) => panic!("faulted: {fault:?}"),
        }
    }
    panic!("still running after 1000 resumes");
}

/// Runs the free function `name` of `program` on `args`.
fn run_fn(program: &Program, name: &str, args: &[Value]) -> Value {
    let machine = Machine::new(program, name, args)
        .unwrap_or_else(|| panic!("`{name}` is a function taking {} argument(s)", args.len()));
    finish(machine, program, &mut Host::new())
}

/// The text the free function `name` of `program` returns on `args`.
fn text_of(program: &Program, name: &str, args: &[Value]) -> String {
    let mut machine = Machine::new(program, name, args)
        .unwrap_or_else(|| panic!("`{name}` is a function taking {} argument(s)", args.len()));
    let mut host = Host::new();
    for _ in 0..1000 {
        match machine.run(program, &mut host, u64::MAX) {
            Run::Completed => {
                return machine
                    .resolve_str(machine.result(), program, &host)
                    .unwrap_or_else(|fault| panic!("`{name}` returned no text: {fault:?}"))
                    .to_owned();
            }
            Run::Suspended(_) => host.awaiting = None,
            Run::Faulted(fault) => panic!("`{name}` faulted: {fault:?}"),
        }
    }
    panic!("`{name}` still running after 1000 resumes");
}
