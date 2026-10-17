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

//! Named arguments and parameter defaults: `F(x, speed: 2.0)`,
//! `fn int F(int x, float speed = 1.0)`.
//!
//! An argument starting with `name:` fills the parameter of that name.
//! Positional arguments come first, filling parameters in order; named ones
//! follow, in any order. A parameter is given once, a name must be one of the
//! callee's, and every parameter without a default must be given. A default is
//! a constant, and a parameter with one is followed only by parameters with
//! one; a lifecycle method, a handler and an event take none.
//!
//! What takes names: a script `fn`, a behavior method, a state method, an
//! engine function (its Rust parameter names), an engine constructor (its
//! fields), an event's payload, and `become`'s state data — which is now
//! checked against the state's parameters, labelled or not. Array methods, the
//! component operations and `Spawn` take none.
//!
//! Arguments are evaluated in the order they are written, whatever order the
//! parameters are declared in. A call written without names compiles exactly
//! as it did before named arguments existed.

mod calls;
mod defaults;
mod field_defaults;
mod refusals;
mod resume;
mod unchanged;

use khora_core::ecs::entity::EntityId;
use khora_core::script::ScriptEvent;
use khora_script::arena::Persisted;
use khora_script::dispatch::{deliver, resolve_member};
use khora_script::vm::{Machine, Program, Run, Value};
use khora_script::{compile_module, CompileOutcome, Diagnostic, Function, Host, MemoryLoader};

use crate::support::fresh_store;

/// The entity a behavior instance runs as.
const SUBJECT: EntityId = EntityId {
    index: 3,
    generation: 1,
};

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

/// `source`, compiled; panics naming every error when it does not compile.
fn build(source: &str) -> Program {
    let result = compile(source);
    let found: Vec<String> = errors(&result)
        .into_iter()
        .map(|d| format!("{} @ {:?}", said(d), underlined(source, d)))
        .collect();
    assert!(found.is_empty(), "should compile:\n{source}\n{found:?}");
    result.program.expect("a program with no errors")
}

/// Whether `text` holds `fragment`; a fragment may list `|`-separated
/// alternatives, one of which must be there.
fn holds(text: &str, fragment: &str) -> bool {
    fragment
        .split('|')
        .any(|alternative| text.contains(alternative))
}

/// What `diagnostic` underlines in `source`.
fn underlined<'s>(source: &'s str, diagnostic: &Diagnostic) -> &'s str {
    source
        .get(diagnostic.span.start as usize..diagnostic.span.end as usize)
        .unwrap_or("<out of the file>")
}

/// The refusal of `source` whose line and note say every one of `fragments`;
/// panics when it compiles or when no error says them all.
fn refused_saying(source: &str, fragments: &[&str]) -> Diagnostic {
    let result = compile(source);
    assert!(
        !result.succeeded(),
        "should be refused, and compiled:\n{source}"
    );
    errors(&result)
        .into_iter()
        .find(|d| {
            let text = said(d);
            fragments.iter().all(|fragment| holds(&text, fragment))
        })
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

/// The refusal of `source` saying every one of `fragments` and underlining
/// something inside the first `place` written in it; panics when it compiles
/// or when no error does both.
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
            fragments.iter().all(|fragment| holds(&text, fragment))
                && d.span.start as usize >= start
                && d.span.end as usize <= end
                && d.span.start < d.span.end
        })
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "expected a refusal of\n{source}\nsaying {fragments:?} at `{place}`; got {:?}",
                errors(&result)
                    .into_iter()
                    .map(|d| format!("{} @ {:?}", said(d), underlined(source, d)))
                    .collect::<Vec<_>>()
            )
        })
}

/// Runs `machine` to its end, an `await` treated as elapsed at once.
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

/// What the free function `name` returns, called with `args`.
fn run_fn(program: &Program, name: &str, args: &[Value]) -> Value {
    let machine = Machine::new(program, name, args)
        .unwrap_or_else(|| panic!("`{name}` is a function taking {} argument(s)", args.len()));
    finish(machine, program, &mut Host::new())
}

/// One instance of a behavior, as the engine creates one: its store at its
/// defaults, its first state entered, an entity to be `this`.
struct Instance<'p> {
    program: &'p Program,
    behavior: &'static str,
    host: Host,
}

impl<'p> Instance<'p> {
    fn of(program: &'p Program, behavior: &'static str) -> Self {
        Self {
            program,
            behavior,
            host: Host::new()
                .with_fields(fresh_store(program, behavior))
                .for_entity(SUBJECT),
        }
    }

    /// Calls `member` with no argument, resolved as the engine resolves it —
    /// the current state's first, the behavior's own otherwise.
    fn call(&mut self, member: &str) -> Value {
        let function = resolve_member(self.program, self.behavior, member, &self.host)
            .unwrap_or_else(|| panic!("`{}` has a member `{member}`", self.behavior));
        let machine = Machine::new(self.program, &function, &[])
            .unwrap_or_else(|| panic!("`{function}` takes no argument"));
        finish(machine, self.program, &mut self.host)
    }

    /// What the instance has raised since the last look.
    fn raised(&mut self) -> Vec<ScriptEvent> {
        self.host.take_events().as_slice().to_vec()
    }

    /// Delivers every event raised so far to the instance itself.
    fn deliver_raised(&mut self) {
        for event in self.raised() {
            let delivered = deliver(
                self.program,
                self.behavior,
                &event,
                &mut self.host,
                u64::MAX,
                |id| id == SUBJECT,
            )
            .unwrap_or_else(|why| panic!("`{}` was not delivered: {why}", event.name));
            match (delivered.outcome, delivered.suspended) {
                (Run::Faulted(fault), _) => panic!("`{}` faulted: {fault:?}", event.name),
                (_, Some(machine)) => {
                    self.host.awaiting = None;
                    finish(machine, self.program, &mut self.host);
                }
                _ => {}
            }
        }
    }

    /// The behavior field `name` as it stands.
    fn field(&self, name: &str) -> Value {
        let slot = self
            .program
            .layout(self.behavior)
            .and_then(|layout| layout.slot_of(name))
            .unwrap_or_else(|| panic!("`{}` has a field `{name}`", self.behavior));
        match self.host.fields.get(slot) {
            Some(Persisted::Scalar(value)) => *value,
            other => panic!("`{name}` holds a scalar, found {other:?}"),
        }
    }
}

fn function<'p>(program: &'p Program, name: &str) -> &'p Function {
    program
        .function(name)
        .unwrap_or_else(|| panic!("`{name}` is a function of the program"))
}

/// Every site name of the function `name`, in program-counter order: the
/// statement keys it is made of, as a frame frozen there would name them.
fn site_names(program: &Program, name: &str) -> Vec<String> {
    function(program, name)
        .sites
        .iter()
        .map(|site| site.name.clone())
        .collect()
}
