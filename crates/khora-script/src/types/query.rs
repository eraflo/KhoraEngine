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

//! Queries and systems.
//!
//! A query binds, row by row, the entity and copies of its components; only a
//! system's `ref` binding is written back. A system is a behavior with no
//! subject: what needs the entity a behavior is attached to — `this`, a
//! native reading it — or belongs to one entity's time — a state, `become`,
//! an event, `async` — is refused at the construct, saying a system has none.

use std::collections::{HashMap, HashSet};

use super::{Checker, Context, Ty};
use crate::ast::{BehaviorDecl, BehaviorMember, Block, QueryBinding, TypeRef};
use crate::diagnostics::Span;

/// The calls among free functions, and those of systems to free functions:
/// a free function reading the subject — itself or through those it calls —
/// is refused where a system calls it.
#[derive(Debug, Default)]
pub struct SubjectCalls {
    /// Each free function's calls to free functions.
    calls: HashMap<String, Vec<String>>,
    /// The free functions calling a native that reads the subject, with it.
    reads: HashMap<String, String>,
    /// Each call a system makes to a free function: callee, call.
    from_systems: Vec<(String, Span)>,
}

/// How a query names a component.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Named {
    /// Bound, every row holding it.
    Required,
    /// Bound as `T?`.
    Optional,
    /// After `with`.
    With,
    /// After `without`.
    Without,
}

impl Checker {
    /// A `system`: checked as a behavior, with what it has no subject for
    /// refused.
    pub(super) fn check_system(&mut self, decl: &BehaviorDecl) {
        for member in &decl.members {
            match member {
                BehaviorMember::State(state) => self.error_note(
                    format!(
                        "a system has no states: `state {}` belongs to a behavior",
                        state.name
                    ),
                    state.name_span,
                    "a system is one rule over the entities its queries match; what one entity is doing is a behavior's state",
                ),
                BehaviorMember::Handler(handler) => self.error_note(
                    format!(
                        "a system receives no event: `on {}` is aimed at an entity",
                        handler.event
                    ),
                    handler.event_span,
                    "handle it in a behavior of the entities it is sent to",
                ),
                BehaviorMember::Method(method) if method.is_async => self.error_note(
                    format!(
                        "a system has no `async` member: `{}` would wait for one entity's time",
                        method.name
                    ),
                    method.name_span,
                    "a system's time is `every` and `after`",
                ),
                BehaviorMember::Method(method)
                    if matches!(method.name.as_str(), "OnSpawn" | "OnDespawn") =>
                {
                    self.error_note(
                        format!("a system has no entity to call `{}` for", method.name),
                        method.name_span,
                        "a system starts with the game: of the engine's calls it takes `Update`, `OnLoad` and `OnResumeFailed`",
                    );
                }
                _ => {}
            }
        }
        let outer = self.system.replace(decl.name.clone());
        self.check_behavior(decl);
        self.system = outer;
    }

    /// Records a call to `name` at `span`, for [`report_subject_calls`].
    ///
    /// [`report_subject_calls`]: Self::report_subject_calls
    pub(super) fn note_call(&mut self, name: &str, span: Span) {
        let free = self.free_functions.contains(name) && !self.method_names.contains(name);
        if let Some(caller) = self.free_function.clone() {
            if free {
                self.subject_calls
                    .calls
                    .entry(caller)
                    .or_default()
                    .push(name.to_owned());
            } else if crate::native::world::SUBJECT_NATIVES.contains(&name) {
                self.subject_calls
                    .reads
                    .entry(caller)
                    .or_insert_with(|| name.to_owned());
            }
        } else if self.system.is_some() && free {
            self.subject_calls
                .from_systems
                .push((name.to_owned(), span));
        }
    }

    /// Refuses each call a system makes to a free function that reads the
    /// subject, itself or through any chain of free functions.
    pub(super) fn report_subject_calls(&mut self) {
        let calls = std::mem::take(&mut self.subject_calls);
        for (callee, span) in &calls.from_systems {
            let Some(native) = reading(&calls, callee) else {
                continue;
            };
            self.error_note(
                format!(
                    "`{callee}` reads the entity a behavior is attached to through `{native}()`, and a system has none"
                ),
                *span,
                "a system's entities are the rows of its queries — pass the function an `Entity`",
            );
        }
    }

    /// `foreach (Entity e, ref Health h, Regen? r) with A without B { … }`.
    pub(super) fn check_query(
        &mut self,
        bindings: &[QueryBinding],
        with: &[(String, Span)],
        without: &[(String, Span)],
        body: &Block,
        span: Span,
        context: &Context,
    ) {
        let mut named: Vec<(String, Named)> = Vec::new();
        let mut entity = false;
        self.scopes.push();

        for binding in bindings {
            let simple = match &binding.ty {
                TypeRef::Named { name, .. } => Some((name.as_str(), false)),
                TypeRef::Optional { inner, .. } => match &**inner {
                    TypeRef::Named { name, .. } => Some((name.as_str(), true)),
                    _ => None,
                },
                _ => None,
            };
            let Some((name, optional)) = simple else {
                // `Health[]`, `Health??`: no row holds one.
                self.error_note(
                    format!(
                        "`{}` is not a component",
                        crate::bytecode::keys::type_name(&binding.ty)
                    ),
                    binding.span,
                    "a query binds `Entity` and components, and filters with components",
                );
                self.scopes
                    .declare_query(&binding.name, Ty::Error, binding.span, binding.write);
                continue;
            };
            let ty = if name == "Entity" {
                self.check_entity_binding(binding, optional, &mut entity);
                Ty::Entity
            } else if self.query_component(name, binding.span, &mut named, kind(optional)) {
                if binding.write {
                    self.check_ref_binding(name, optional, binding.span);
                }
                let copy = Ty::Struct(name.to_owned());
                if optional {
                    Ty::Optional(Box::new(copy))
                } else {
                    copy
                }
            } else {
                Ty::Error
            };
            if self
                .scopes
                .declare_query(&binding.name, ty, binding.span, binding.write)
                .is_some()
            {
                self.error(
                    format!("`{}` is bound twice in this query", binding.name),
                    binding.span,
                );
            }
        }
        for (filter, names) in [(Named::With, with), (Named::Without, without)] {
            for (name, at) in names {
                self.query_component(name, *at, &mut named, filter);
            }
        }
        if !named
            .iter()
            .any(|(_, how)| matches!(how, Named::Required | Named::With))
        {
            self.error_note(
                "a query needs a component every row holds",
                span,
                "bind one without `?`, or name one after `with`",
            );
        }

        let looping = Context {
            in_loop: true,
            ..context.clone()
        };
        for statement in &body.statements {
            self.check_stmt(statement, &looping);
        }
        self.scopes.pop();
    }

    /// `Entity e`: the row's entity, always there, bound once.
    fn check_entity_binding(&mut self, binding: &QueryBinding, optional: bool, bound: &mut bool) {
        if optional || binding.write {
            self.error_note(
                "a row's entity is always there, and is not written",
                binding.span,
                format!("bind it as `Entity {}`", binding.name),
            );
        }
        if *bound {
            self.error(
                "the row's entity is bound twice in this query",
                binding.span,
            );
        }
        *bound = true;
    }

    /// A `ref` binding: a system's, of a component that is not a behavior's,
    /// never optional.
    fn check_ref_binding(&mut self, name: &str, optional: bool, span: Span) {
        if self.system.is_none() {
            self.error_note(
                format!("this query is read-only: only a system writes `{name}` in place"),
                span,
                format!(
                    "bind `{name}` without `ref`, and write an entity's component with `e.Set({name} {{ … }})`"
                ),
            );
        } else if self.behaviors.iter().any(|behavior| behavior == name) {
            self.error_note(
                format!(
                    "`{name}` is a behavior's component: its fields are written by its own turn, or by `Set`"
                ),
                span,
                format!("bind it without `ref`, and write it with `e.Set({name} {{ … }})`"),
            );
        } else if optional {
            self.error_note(
                format!(
                    "a `ref` binding cannot be optional: where `{name}` is absent there is nothing to write back to"
                ),
                span,
                format!(
                    "bind `ref {name}` to visit only its holders, or read `{name}?` and write with `Set`"
                ),
            );
        }
    }

    /// Records that the query names component `name` as `how`, refusing a
    /// name that is not a component and one named before. `true` when it is
    /// a component named for the first time.
    fn query_component(
        &mut self,
        name: &str,
        span: Span,
        named: &mut Vec<(String, Named)>,
        how: Named,
    ) -> bool {
        if !self.components.contains_key(name) {
            self.error_note(
                format!("`{name}` is not a component"),
                span,
                "a query binds `Entity` and components, and filters with components",
            );
            return false;
        }
        if let Some((_, before)) = named.iter().find(|(known, _)| known == name) {
            if (*before == Named::Without) != (how == Named::Without) {
                let note = if matches!(*before, Named::Optional) || how == Named::Optional {
                    "its binding would always be `null`: drop one of the two"
                } else {
                    "no row could match it: drop one of the two"
                };
                self.error_note(
                    format!("`{name}` is both named and excluded by this query"),
                    span,
                    note,
                );
            } else {
                self.error(format!("`{name}` is named twice in this query"), span);
            }
            return false;
        }
        named.push((name.to_owned(), how));
        true
    }
}

/// How a binding names its component.
fn kind(optional: bool) -> Named {
    if optional {
        Named::Optional
    } else {
        Named::Required
    }
}

/// The subject-reading native `function` reaches, itself or through the free
/// functions it calls — each followed once, so a cycle ends.
fn reading<'c>(calls: &'c SubjectCalls, function: &str) -> Option<&'c str> {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut next = vec![function];
    while let Some(current) = next.pop() {
        if !seen.insert(current) {
            continue;
        }
        if let Some(native) = calls.reads.get(current) {
            return Some(native);
        }
        if let Some(callees) = calls.calls.get(current) {
            next.extend(callees.iter().map(String::as_str));
        }
    }
    None
}
