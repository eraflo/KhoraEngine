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

//! How the script lane schedules a behavior or a system, as its author
//! declares it: `[Critical]` served first from the frame's fuel, `[Budget(d)]`
//! a per-frame ceiling, `[Rate(f)]` at most `f` turns a second.
//!
//! The author says what matters and how often; the lane decides who runs
//! when. None of them changes what a turn does — only whether it runs this
//! frame, and with how much of the fuel.

use crate::ast::{Attribute, BehaviorDecl, BehaviorMember, Expr};
use crate::diagnostics::Diagnostic;
use crate::vm::Schedule;

/// `[Critical]`.
pub const CRITICAL: &str = "Critical";
/// `[Budget(0.2ms)]`.
pub const BUDGET: &str = "Budget";
/// `[Rate(10hz)]`.
pub const RATE: &str = "Rate";

/// The scheduling attributes.
pub const SCHEDULING: [&str; 3] = [CRITICAL, BUDGET, RATE];

/// The schedule `attributes` declare — what the lane reads. An argument
/// refused by [`report_schedule`] is left out.
pub fn schedule_of(attributes: &[Attribute]) -> Schedule {
    let first = |name: &str| attributes.iter().find(|attribute| attribute.name == name);
    Schedule {
        critical: first(CRITICAL)
            .is_some_and(|attribute| attribute.args.is_empty() && !attribute.parenthesized),
        budget: first(BUDGET).and_then(|attribute| match attribute.args.as_slice() {
            [Expr::Duration { value, .. }] if *value > 0.0 && value.is_finite() => Some(*value),
            _ => None,
        }),
        rate: first(RATE).and_then(|attribute| match attribute.args.as_slice() {
            [Expr::Frequency { value, .. }] if *value > 0.0 && value.is_finite() => Some(*value),
            _ => None,
        }),
    }
}

/// Every refusal about `decl`'s scheduling attributes: an argument of the
/// wrong kind, one given twice, a rate on a behavior with a fixed-step
/// member — and, on a system, any attribute but these three.
pub fn report_schedule(decl: &BehaviorDecl, system: bool, diagnostics: &mut Vec<Diagnostic>) {
    if system {
        for attribute in &decl.attributes {
            if !SCHEDULING.contains(&attribute.name.as_str()) {
                diagnostics.push(
                    Diagnostic::error(
                        format!("`[{}]` is not an attribute of a system", attribute.name),
                        attribute.span,
                    )
                    .with_note(
                        "a system takes `[Critical]`, `[Budget(…)]` and `[Rate(…)]`; its fields live in `Script`",
                    ),
                );
            }
        }
    }

    for name in SCHEDULING {
        let mut given = decl
            .attributes
            .iter()
            .filter(|attribute| attribute.name == name);
        if let Some(attribute) = given.next() {
            report_arguments(attribute, diagnostics);
        }
        for twice in given {
            diagnostics.push(
                Diagnostic::error(format!("`[{name}]` is given twice"), twice.span)
                    .with_note(format!("a behavior takes `[{name}]` once — keep one")),
            );
        }
    }

    let rated = decl
        .attributes
        .iter()
        .any(|attribute| attribute.name == RATE);
    if rated && declares_fixed_update(&decl.members) {
        let at = decl
            .attributes
            .iter()
            .find(|attribute| attribute.name == RATE)
            .map_or(decl.name_span, |attribute| attribute.span);
        diagnostics.push(
            Diagnostic::error(
                format!(
                    "`{}` declares `FixedUpdate`: a fixed-step member runs every step, and `[Rate(…)]` would skip some",
                    decl.name
                ),
                at,
            )
            .with_note("drop the rate, or move the fixed-step work to its own behavior"),
        );
    }
}

fn report_arguments(attribute: &Attribute, diagnostics: &mut Vec<Diagnostic>) {
    let (fits, message, note) = match attribute.name.as_str() {
        CRITICAL => (
            attribute.args.is_empty() && !attribute.parenthesized,
            "`[Critical]` takes no argument",
            "write `[Critical]`: the behavior is served first from the frame's fuel",
        ),
        BUDGET => (
            matches!(attribute.args.as_slice(),
                [Expr::Duration { value, .. }] if *value > 0.0 && value.is_finite()),
            "`[Budget(…)]` takes one positive duration: `[Budget(0.2ms)]`",
            "the most of a frame the behavior may spend, as a duration literal — `0.2ms`, `1ms`",
        ),
        RATE => (
            matches!(attribute.args.as_slice(),
                [Expr::Frequency { value, .. }] if *value > 0.0 && value.is_finite()),
            "`[Rate(…)]` takes one positive frequency: `[Rate(10hz)]`",
            "how many turns a second an instance takes at most, as a frequency literal — `10hz`, `2.5hz`",
        ),
        _ => return,
    };
    if !fits {
        diagnostics.push(Diagnostic::error(message, attribute.span).with_note(note));
    }
}

/// Whether `members` — the behavior's, or a state's — declare `FixedUpdate`.
fn declares_fixed_update(members: &[BehaviorMember]) -> bool {
    members.iter().any(|member| match member {
        BehaviorMember::Method(method) => method.name == crate::lifecycle::FIXED_UPDATE.name,
        BehaviorMember::State(state) => declares_fixed_update(&state.members),
        _ => false,
    })
}
