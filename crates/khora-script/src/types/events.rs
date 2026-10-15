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

//! Events: `event Damaged(int amount, Entity source);`, raised with
//! `target.Raise(Damaged(3, this))` and taken with `on Damaged(int a, Entity s)`.
//!
//! The declaration is the event's identity and its signature, and both ends
//! are held to it here — a raise of an event that does not exist, or with the
//! wrong payload, and a handler of one, are compile errors rather than a
//! delivery that silently finds nobody or faults its receiver.

use crate::ast::{EventDecl, Expr, HandlerDecl};
use crate::diagnostics::Span;

use super::{Checker, Context, Ty};

/// What an event declares, gathered in the first pass.
#[derive(Debug, Clone)]
pub struct EventInfo {
    /// What it carries, in order.
    pub params: Vec<Ty>,
}

impl EventInfo {
    /// `Damaged(int, Entity)` — as a message names it.
    pub fn signature(&self, name: &str) -> String {
        format!(
            "{name}({})",
            self.params
                .iter()
                .map(Ty::name)
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

impl Checker {
    /// The events the engine raises: in scope in every module, whether or not
    /// it was compiled with the module serving them.
    pub(super) fn collect_engine_events(&mut self) {
        for event in khora_core::script::ENGINE_EVENTS {
            let params = event
                .params
                .iter()
                .map(|(ty, name)| {
                    super::ty::resolve(
                        &crate::ast::TypeRef::Named {
                            name: (*ty).to_owned(),
                            span: Span::empty(0),
                        },
                        &|_| false,
                    )
                    .unwrap_or_else(|| panic!("the engine event's `{name}` has a type Ergon names"))
                })
                .collect();
            self.events
                .insert(event.name.to_owned(), EventInfo { params });
        }
    }

    /// Records `decl`, refusing a name the engine or the language owns and a
    /// payload the delivery could not carry.
    pub(super) fn collect_event(&mut self, decl: &EventDecl) {
        let name = &decl.name;
        if khora_core::script::ENGINE_EVENTS
            .iter()
            .any(|event| event.name == name)
        {
            self.error_note(
                format!("`{name}` is an event the engine raises"),
                decl.name_span,
                "handle it with `on` — it is declared by the engine and in scope everywhere; choose another name for your own",
            );
            return;
        }
        if super::components::RESERVED.contains(&name.as_str()) || name == "Raise" {
            self.error_note(
                format!("`{name}` is reserved"),
                decl.name_span,
                "the language names its operations on entities with it — `e.Set(…)`, `e.Raise(…)`, `Spawn(…)` — choose another name for the event",
            );
        } else if let Some(hook) = crate::lifecycle::of(name) {
            self.error_note(
                format!("`{name}` is a member the engine calls, not an event"),
                decl.name_span,
                format!(
                    "a raise of `{}` would run that member — name the event after what happened",
                    hook.name
                ),
            );
        } else if name.starts_with("__") {
            self.error_note(
                format!("`{name}` starts with `__`, which the compiler keeps for itself"),
                decl.name_span,
                "a state's entry and a timer's body are named that way — choose another name",
            );
        } else if self.natives.contains_key(name) {
            self.error_note(
                format!("`{name}` is already an engine function"),
                decl.name_span,
                "rename the event; a raise and a call would read alike",
            );
        } else if super::ty::resolve(
            &crate::ast::TypeRef::Named {
                name: name.clone(),
                span: decl.name_span,
            },
            &|_| false,
        )
        .is_some()
        {
            self.error_note(
                format!("`{name}` is already a type"),
                decl.name_span,
                "an event's name is global — choose one no type has",
            );
        }

        let mut params = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        for param in &decl.params {
            if seen.contains(&param.name.as_str()) {
                self.error(
                    format!("`{}` names two parameters of `{name}`", param.name),
                    param.span,
                );
            }
            seen.push(&param.name);
            let ty = self.resolve(&param.ty);
            if let Some(why) = uncarried(&ty) {
                self.error_note(
                    format!("an event cannot carry {why} — `{}`", ty.name()),
                    param.span,
                    "an event carries bool, int, float, string, Vec2, Vec3, Vec4, Quat, Color, Entity, Duration or Angle; send what the handler needs to look up",
                );
            }
            params.push(ty);
        }
        self.events.insert(name.clone(), EventInfo { params });
    }

    /// `target.Raise(E(args))`: `E` an event in scope, its arguments its
    /// payload.
    pub(super) fn check_raise(&mut self, args: &[Expr], at: Span, context: &Context) -> Ty {
        let construction = match args {
            [Expr::Call { callee, args, span }] => match callee.as_ref() {
                Expr::Ident { name, .. } => Some((name.clone(), args, *span)),
                _ => None,
            },
            _ => None,
        };
        let Some((event, arguments, span)) = construction else {
            if let Some(Expr::Str { value, .. }) = args.first() {
                self.raise_by_name(value, &args[1..], at, context);
                return Ty::Void;
            }
            for argument in args {
                self.check_expr(argument, context);
            }
            self.error_note(
                "`Raise` takes an event: `target.Raise(Damaged(3))`",
                at,
                "declare it with `event Damaged(int amount);`, then raise it by name with its payload",
            );
            return Ty::Void;
        };
        let Some(info) = self.events.get(&event).cloned() else {
            for argument in arguments {
                self.check_expr(argument, context);
            }
            self.unknown_event(&event, span);
            return Ty::Void;
        };
        if arguments.len() != info.params.len() {
            self.error_note(
                format!(
                    "`{event}` carries {} value{}, found {}",
                    info.params.len(),
                    if info.params.len() == 1 { "" } else { "s" },
                    arguments.len()
                ),
                span,
                format!("it is declared `{}`", info.signature(&event)),
            );
        }
        for (argument, expected) in arguments.iter().zip(&info.params) {
            let actual = self.check_expr(argument, context);
            self.expect_assignable(expected, &actual, argument.span());
        }
        for extra in arguments.iter().skip(info.params.len()) {
            self.check_expr(extra, context);
        }
        Ty::Void
    }

    /// The string form, `Raise("Damaged", 3)`: refused with the declaration
    /// and the raise to write instead.
    fn raise_by_name(&mut self, name: &str, payload: &[Expr], at: Span, context: &Context) {
        let types: Vec<Ty> = payload
            .iter()
            .map(|arg| self.check_expr(arg, context))
            .collect();
        let declared = types
            .iter()
            .enumerate()
            .map(|(i, ty)| format!("{} value{i}", ty.name()))
            .collect::<Vec<_>>()
            .join(", ");
        let written = payload
            .iter()
            .map(super::components::sketch)
            .collect::<Vec<_>>()
            .join(", ");
        self.error_note(
            format!("an event is raised by its declaration, not by its name: declare `event {name}({declared});` and write `Raise({name}({written}))`"),
            at,
            "the raise and every handler are then checked against the declaration — a typo or a wrong payload stops the compile instead of a delivery",
        );
    }

    /// `Raise(target, E(…))` — the free form, refused with its rewrite.
    pub(super) fn refuse_free_raise(&mut self, args: &[Expr], span: Span, context: &Context) -> Ty {
        let rewrite = match args.split_first() {
            Some((receiver, rest)) => format!(
                "{}.Raise({})",
                super::components::sketch_receiver(receiver),
                rest.iter()
                    .map(super::components::sketch)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            None => "this.Raise(…)".to_owned(),
        };
        for argument in args {
            if !matches!(argument, Expr::Call { callee, .. } if matches!(callee.as_ref(), Expr::Ident { name, .. } if self.events.contains_key(name)))
            {
                self.check_expr(argument, context);
            }
        }
        self.error_note(
            format!("Write `{rewrite}`"),
            span,
            "`Raise` is called on the entity it acts on",
        );
        Ty::Error
    }

    /// `E(args)` anywhere but a raise's argument.
    pub(super) fn refuse_construction(&mut self, event: &str, span: Span) {
        self.error_note(
            format!("`{event}` is an event: it is raised, not stored"),
            span,
            format!("write `target.Raise({event}(…))`"),
        );
    }

    /// `on E(…)`: `E` an event in scope, the handler taking all of its
    /// payload or none of it.
    pub(super) fn check_handler_signature(&mut self, handler: &HandlerDecl) {
        let Some(info) = self.events.get(&handler.event).cloned() else {
            self.unknown_event(&handler.event, handler.event_span);
            return;
        };
        if handler.params.is_empty() {
            return;
        }
        let taken: Vec<Ty> = handler
            .params
            .iter()
            .map(|param| self.resolve(&param.ty))
            .collect();
        let fits = taken.len() == info.params.len()
            && taken.iter().zip(&info.params).all(|(taken, declared)| {
                taken == declared || *taken == Ty::Error || *declared == Ty::Error
            });
        if !fits {
            self.error_note(
                format!(
                    "a handler of `{}` takes what the event carries, or nothing",
                    handler.event
                ),
                handler.event_span,
                format!(
                    "the event is declared `{}` — take its parameters in that order, or write `on {} {{ … }}`",
                    info.signature(&handler.event),
                    handler.event
                ),
            );
        }
    }

    fn unknown_event(&mut self, event: &str, span: Span) {
        let note = if self.structs.contains_key(event)
            || self.components.contains_key(event)
            || self.functions.contains_key(event)
        {
            format!("`{event}` is declared, but not as an event")
        } else {
            format!("declare it with `event {event}(…);`, or import the module declaring it")
        };
        self.error_note(format!("no event named `{event}`"), span, note);
    }
}

/// Why a value of `ty` cannot ride an event, if it cannot.
fn uncarried(ty: &Ty) -> Option<&'static str> {
    match ty {
        Ty::Optional(_) => Some("an optional"),
        Ty::Array(_) => Some("an array"),
        Ty::Map(..) => Some("a map"),
        Ty::Struct(_) => Some("a struct"),
        Ty::Behavior(_) => Some("a component"),
        Ty::Void => Some("nothing"),
        _ => None,
    }
}
