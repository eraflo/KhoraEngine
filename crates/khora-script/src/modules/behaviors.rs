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

//! The components a project's behaviors are.
//!
//! A `behavior` is stored as two components. Its own — `Guard` — holds the
//! fields its author sets, in the behavior's `[Domain(…)]`, and is saved by a
//! scene. Its **state companion** — `Guard#state` — holds what the engine
//! writes while it runs: the current state by name, that state's data, the
//! timers, a suspended sequence, the lifecycle; it is runtime state a game
//! save keeps and a scene never does, always in the `Script` domain, with the
//! same fields for every behavior. `#` is no part of a name a script can
//! write, so no script names a companion.

use khora_core::ecs::SemanticDomain;
use khora_core::script::{ErgonType, ScriptValue};

use super::components::{
    domain_of, ergon_type, known_domains, non_finite, with_struct_fields, DOMAIN,
};
use super::{Resolved, ResolvedModule};
use crate::ast::{Attribute, BehaviorDecl, BehaviorMember, Expr, Field, Item};
use crate::diagnostics::Diagnostic;
use crate::vm::{DeclaredComponent, DeclaredField, DeclaredKind, Program};

/// What separates a behavior's name from its state companion's.
pub const STATE_SUFFIX: &str = "#state";

/// The name of the state companion of `behavior`.
pub fn state_companion(behavior: &str) -> String {
    format!("{behavior}{STATE_SUFFIX}")
}

/// Every refusal about the behaviors the project's own modules declare: a
/// name an engine component has, a `[Domain(…)]` that names no domain or is
/// given twice.
pub fn report_script_behaviors(resolved: &Resolved, engine: &[String]) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (_, decl, declared) in script_behaviors(resolved) {
        if engine.contains(&decl.name) {
            let what = declared.word();
            diagnostics.push(
                Diagnostic::error(
                    format!(
                        "`{}` is an engine component. Choose another name for the {what}.",
                        decl.name
                    ),
                    decl.name_span,
                )
                .with_note(format!("a {what} is a component: its name is unique among every component the engine stores")),
            );
        }
        if !declared.system {
            report_domain(&decl.attributes, &mut diagnostics);
        }
        super::schedule::report_schedule(decl, declared.system, &mut diagnostics);
        for field in fields_of(decl) {
            if let Some(span) = field.default.as_ref().and_then(non_finite) {
                diagnostics.push(
                    Diagnostic::error("this number is beyond what a float holds", span)
                        .with_note("a float holds up to about 3.4e38: a larger literal reads as infinity, which no field stores"),
                );
            }
        }
    }
    diagnostics
}

/// The attributes a behavior takes: where its fields live, and how the
/// scheduler treats it.
const BEHAVIOR_ATTRIBUTES: [&str; 4] = [DOMAIN, "Critical", "Budget", "Rate"];

/// One `[Domain(X)]` at most, naming a domain; a behavior's other attributes
/// are the scheduler's.
fn report_domain(attributes: &[Attribute], diagnostics: &mut Vec<Diagnostic>) {
    // A misspelt attribute would otherwise be silently ignored — and
    // `[Domian(Physics)]` would leave the fields in `Script`.
    for attribute in attributes {
        if !BEHAVIOR_ATTRIBUTES.contains(&attribute.name.as_str()) {
            diagnostics.push(
                Diagnostic::error(
                    format!("`[{}]` is not an attribute of a behavior", attribute.name),
                    attribute.span,
                )
                .with_note(
                    "a behavior takes `[Domain(…)]`, `[Critical]`, `[Budget(…)]` and `[Rate(…)]`",
                ),
            );
        }
    }
    let mut placed = false;
    for attribute in attributes
        .iter()
        .filter(|attribute| attribute.name == DOMAIN)
    {
        if placed {
            diagnostics.push(
                Diagnostic::error(
                    "a behavior's fields live in one domain: `[Domain]` is given twice",
                    attribute.span,
                )
                .with_note("keep the one that says where its data belongs"),
            );
            continue;
        }
        placed = true;
        match attribute.args.as_slice() {
            [Expr::Ident { name, span }] if SemanticDomain::from_name(name).is_none() => {
                diagnostics.push(
                    Diagnostic::error(format!("`{name}` is not a domain"), *span)
                        .with_note(format!("`[Domain(…)]` names one of {}", known_domains())),
                );
            }
            [Expr::Ident { .. }] => {}
            _ => diagnostics.push(
                Diagnostic::error("`[Domain(…)]` takes one domain's name", attribute.span)
                    .with_note(format!(
                        "write `[Domain(Spatial)]` — one of {}",
                        known_domains()
                    )),
            ),
        }
    }
}

/// The two components each behavior and system of the project's own modules
/// is: its fields, and its state companion. Each carries the item's place
/// among the module's behaviors and systems, in source order: the order they
/// run in.
pub fn declared_behaviors(resolved: &Resolved, program: &Program) -> Vec<DeclaredComponent> {
    script_behaviors(resolved)
        .flat_map(|(module, decl, declared)| {
            let order = declared.order;
            let kind = if declared.system {
                DeclaredKind::System
            } else {
                DeclaredKind::Behavior
            };
            [
                DeclaredComponent {
                    name: decl.name.clone(),
                    module: module.path.clone(),
                    domain: domain_of(&decl.attributes).unwrap_or(SemanticDomain::Script),
                    fields: fields_of(decl)
                        .enumerate()
                        .map(|(slot, field)| declared_field(field, slot, &decl.name, program))
                        .collect(),
                    kind,
                    order,
                },
                DeclaredComponent {
                    name: state_companion(&decl.name),
                    module: module.path.clone(),
                    domain: SemanticDomain::Script,
                    fields: companion_fields(),
                    kind: DeclaredKind::BehaviorState {
                        behavior: decl.name.clone(),
                    },
                    order,
                },
            ]
        })
        .collect()
}

/// What a behavior-shaped item of a module is.
#[derive(Clone, Copy)]
struct Declared {
    /// A `system` rather than a `behavior`.
    system: bool,
    /// Its place among the module's behaviors and systems, in source order.
    order: u32,
}

impl Declared {
    /// What an author calls it.
    fn word(self) -> &'static str {
        if self.system {
            "system"
        } else {
            "behavior"
        }
    }
}

/// Every `behavior` and `system` written outside the engine's module, with its
/// module.
fn script_behaviors(
    resolved: &Resolved,
) -> impl Iterator<Item = (&ResolvedModule, &BehaviorDecl, Declared)> {
    resolved
        .modules
        .iter()
        .filter(|module| module.path != crate::ENGINE_COMPONENTS_MODULE)
        .flat_map(|module| {
            module
                .module
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::Behavior(decl) => Some((decl, false)),
                    Item::System(decl) => Some((decl, true)),
                    _ => None,
                })
                .enumerate()
                .map(move |(order, (decl, system))| {
                    let order = u32::try_from(order).unwrap_or(u32::MAX);
                    (module, decl, Declared { system, order })
                })
        })
}

/// A behavior's own fields — not a state's — in declaration order.
fn fields_of(decl: &BehaviorDecl) -> impl Iterator<Item = &Field> {
    decl.members.iter().filter_map(|member| match member {
        BehaviorMember::Field(field) => Some(field),
        _ => None,
    })
}

/// A behavior field as its component holds it: its registered default is the
/// layout's — the constant written, or its type's neutral value — so what a
/// row starts at and what a fresh instance holds are one value.
fn declared_field(field: &Field, slot: usize, behavior: &str, program: &Program) -> DeclaredField {
    // A type the storage names no type for — a `Map` — is held as any value.
    let ty = ergon_type(&field.ty).map_or(ErgonType::Any, |ty| {
        with_struct_fields(ty, program, &mut Vec::new())
    });
    let default = program
        .layout(behavior)
        .and_then(|layout| layout.defaults.get(slot))
        .and_then(|owned| crate::bridge::from_owned(owned).ok())
        .map(|value| ty.widened(value))
        .unwrap_or(ScriptValue::Null);
    DeclaredField {
        name: field.name.clone(),
        ty,
        default,
    }
}

/// The fields of every state companion: the current state, by name; that
/// state's data, by name; each timer's remaining time; a suspended sequence;
/// whether the instance spawned; its fault; the bodies a resume abandoned.
fn companion_fields() -> Vec<DeclaredField> {
    let field = |name: &str, ty: ErgonType, default: ScriptValue| DeclaredField {
        name: name.to_owned(),
        ty,
        default,
    };
    let text = || ErgonType::Optional(Box::new(ErgonType::Str));
    vec![
        field("state", text(), ScriptValue::Null),
        field(
            "state_fields",
            ErgonType::Any,
            ScriptValue::Struct(Vec::new()),
        ),
        field("timers", ErgonType::Any, ScriptValue::Array(Vec::new())),
        field("pending", ErgonType::Any, ScriptValue::Null),
        field("spawned", ErgonType::Bool, ScriptValue::Bool(false)),
        field("fault", ErgonType::Any, ScriptValue::Null),
        field(
            "resume_failed",
            ErgonType::Any,
            ScriptValue::Array(Vec::new()),
        ),
        field("owed", ErgonType::Float, ScriptValue::Float(0.0)),
        field(
            "fixed_owed",
            ErgonType::Array(Box::new(ErgonType::Float)),
            ScriptValue::Array(Vec::new()),
        ),
    ]
}
