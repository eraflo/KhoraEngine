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

//! Naming the places a frame can stand.
//!
//! A site is named from the source's structure, never from a counter over the
//! function, so an edit elsewhere leaves it alone:
//!
//! ```text
//! site   := "entry" | path [ ":" point ]
//! path   := step ( "/" branch "/" step )*
//! step   := kind "." hash8 [ "#" n ]
//! branch := "then" | "else" | "body" | "arm." hash8 [ "#" n ]
//! point  := "head" | "await" [ "#" n ] | "call." callee [ "#" n ]
//! ```
//!
//! `hash8` is what the statement says ([`keys`](super::keys)) — for a `match`
//! arm, what its pattern says; `#n` tells apart siblings that say the same
//! thing, the awaits of one statement, or its calls to one callee, and is left
//! out when zero.

use std::collections::HashMap;

use super::keys::statement_key;
use super::{Compiler, Shape};
use crate::ast::{BehaviorMember, Stmt};
use crate::vm::{Reg, Site, SiteKind, SiteLocal, TimerKind};

/// Where the compiler is, for naming what it emits.
#[derive(Debug, Default)]
pub struct Naming {
    /// The sites recorded so far in the function being compiled.
    pub sites: Vec<Site>,
    /// Steps and branches from the function's body to the statement being
    /// compiled.
    path: Vec<String>,
    /// Per open block, how many siblings each `kind.hash8` has had.
    siblings: Vec<HashMap<String, usize>>,
    /// Per open statement, how many of each point it has had.
    points: Vec<HashMap<String, usize>>,
}

impl Naming {
    /// Starts a function.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// The path to the statement being compiled.
    pub fn path(&self) -> String {
        self.path.join("/")
    }

    /// Opens a block of sibling statements.
    pub fn open_block(&mut self) {
        self.siblings.push(HashMap::new());
    }

    /// Closes the innermost block.
    pub fn close_block(&mut self) {
        self.siblings.pop();
    }

    /// Enters a branch of the statement being compiled.
    pub fn branch(&mut self, name: &str) {
        self.path.push(name.to_owned());
    }

    /// Leaves the innermost branch.
    pub fn leave_branch(&mut self) {
        self.path.pop();
    }

    /// Enters `statement`, naming it among its siblings.
    pub fn enter(&mut self, statement: &Stmt) {
        let (kind, hash) = statement_key(statement);
        self.enter_step(&format!("{kind}.{hash}"));
    }

    /// Enters a step already keyed — a field's default, a state's entry.
    pub fn enter_step(&mut self, step: &str) {
        let seen = self
            .siblings
            .last_mut()
            .map(|block| {
                let count = block.entry(step.to_owned()).or_insert(0);
                *count += 1;
                *count - 1
            })
            .unwrap_or(0);
        self.path.push(ordinal(step, seen));
        self.points.push(HashMap::new());
    }

    /// The block a local declared now belongs to: the path to here, without
    /// the `let` that declares it. A `for`'s own initialiser declares into the
    /// `for`, which stays in the path.
    pub fn scope(&self) -> String {
        let declaring = self
            .path
            .last()
            .is_some_and(|step| step.starts_with("let."));
        let end = self.path.len() - usize::from(declaring);
        self.path[..end].join("/")
    }

    /// Leaves the statement entered last.
    pub fn leave(&mut self) {
        self.path.pop();
        self.points.pop();
    }

    /// The name of the next `point` of the statement being compiled, or `None`
    /// outside any statement.
    pub fn point(&mut self, point: &str) -> Option<String> {
        if self.path.is_empty() {
            return None;
        }
        let count = self.points.last_mut()?.entry(point.to_owned()).or_insert(0);
        let seen = *count;
        *count += 1;
        Some(format!("{}:{}", self.path(), ordinal(point, seen)))
    }
}

/// `name`, then `#n` when `n` is not zero.
fn ordinal(name: &str, n: usize) -> String {
    match n {
        0 => name.to_owned(),
        n => format!("{name}#{n}"),
    }
}

impl Compiler {
    /// The locals in scope, as a site records them.
    fn site_locals(&self) -> Vec<SiteLocal> {
        self.locals
            .iter()
            .map(|local| SiteLocal {
                name: local.name.clone(),
                ty: local.ty.clone(),
                scope: local.scope.clone(),
                register: local.register,
            })
            .collect()
    }

    /// Records a site at the next instruction.
    pub fn record_site(&mut self, name: String, kind: SiteKind, temporaries: Vec<Reg>) {
        let locals = self.site_locals();
        self.naming.sites.push(Site {
            name,
            pc: self.here() as u32,
            kind,
            locals,
            temporaries,
        });
    }

    /// Records the function's entry: its parameters, nothing else.
    pub fn record_entry(&mut self) {
        self.record_site("entry".to_owned(), SiteKind::Entry, Vec::new());
    }

    /// Emits the safepoint a statement starts with, and names it.
    pub fn statement_safepoint(&mut self, kind: SiteKind) {
        let name = self.naming.path();
        self.record_site(name, kind, Vec::new());
        self.emit(crate::vm::Instruction::Safepoint);
    }

    /// Names the place just past an `await` whose result lands in `dst`.
    ///
    /// Every temporary taken so far in the statement is listed, the await's
    /// own destination last: what the rest of the statement may still read.
    pub fn record_await(&mut self) {
        let Some(name) = self.naming.point("await") else {
            return;
        };
        let temporaries = self.registers.live_temporaries();
        self.record_site(name, SiteKind::Await, temporaries);
    }

    /// Names the place just past a call to `callee` whose frame began at
    /// `base` and whose result lands in `dst`.
    ///
    /// The temporaries below the callee's window are the caller's, kept across
    /// the call; the rest of the window was the callee's. The result register
    /// is listed last: a frame stopped here once the call has returned holds
    /// the result there, and the rest of the statement reads it.
    pub fn record_return(&mut self, callee: &str, base: Reg, dst: Reg) {
        let Some(name) = self.naming.point(&format!("call.{callee}")) else {
            return;
        };
        let temporaries = self
            .registers
            .live_temporaries()
            .into_iter()
            .filter(|&register| register < base)
            .chain(std::iter::once(dst))
            .collect();
        self.record_site(name, SiteKind::Return { base, dst }, temporaries);
    }
}

/// The type a site records for a local declared without one: its shape.
pub fn inferred_type(shape: Shape) -> String {
    match shape {
        Shape::Int => "int".to_owned(),
        Shape::Float => "float".to_owned(),
        Shape::Str => "string".to_owned(),
        Shape::Engine(name) => name.to_owned(),
        Shape::Other => "var".to_owned(),
    }
}

/// The function each of `members`' scheduled bodies compiles to, by member
/// index, for the timers `owner` declares (`Guard` or `Guard.Patrol`).
///
/// Named by what the schedule is — `__every(0.5)`, `__after(2)` — and told
/// apart from an identical one by `#n`, the same identity its countdown is
/// saved under. Adding a field, or a schedule of another kind or interval,
/// renames nothing: a body resumed by name always finds its own schedule,
/// never one that took its place.
pub fn timer_names(owner: &str, members: &[BehaviorMember]) -> Vec<(usize, String)> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    members
        .iter()
        .enumerate()
        .filter_map(|(index, member)| {
            let (kind, interval) = match member {
                BehaviorMember::Every(every) => (TimerKind::Every, &every.interval),
                BehaviorMember::After(after) => (TimerKind::After, &after.delay),
                _ => return None,
            };
            let identity = match (kind, literal_seconds(interval)) {
                (TimerKind::Every, Some(seconds)) => format!("__every({seconds})"),
                (TimerKind::After, Some(seconds)) => format!("__after({seconds})"),
                // Refused by `collect_timers`; named only so the function still
                // has one while the error is reported.
                (TimerKind::Every, None) => "__every(?)".to_owned(),
                (TimerKind::After, None) => "__after(?)".to_owned(),
            };
            let count = seen.entry(identity.clone()).or_insert(0);
            let name = format!("{owner}.{}", ordinal(&identity, *count));
            *count += 1;
            Some((index, name))
        })
        .collect()
}

/// The seconds a literal duration expression denotes.
///
/// The lexer has already normalised `500ms` and `2s` to seconds, so this only
/// has to recognise that the expression *is* a literal — a field-driven interval
/// would need evaluating per instance, which a schedule cannot do before
/// deciding whether to fire.
pub fn literal_seconds(expr: &crate::ast::Expr) -> Option<f32> {
    match expr {
        crate::ast::Expr::Duration { value, .. } => Some(*value),
        _ => None,
    }
}
