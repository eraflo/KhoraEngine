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

//! How the inspector shows a field, as its author declares it:
//! `[Range(min, max)]` bounds what editing it reaches, `[Tooltip("…")]` is
//! what hovering its label says, `[Header("…")]` titles the fields from it
//! on.
//!
//! They are the editor's alone: no run time reads them, and a range bounds
//! the inspector, never what a script writes. A component's, a behavior's
//! and a system's fields take them — those the inspector shows; a struct's
//! field and a state's field take none.

use khora_core::script::{FieldHints, FieldRange};

use crate::ast::{Attribute, BehaviorDecl, BehaviorMember, Expr, Field, Item, TypeRef, UnaryOp};
use crate::bytecode::keys::type_name;
use crate::diagnostics::Diagnostic;

use super::Resolved;

/// `[Range(0, 100)]`.
pub const RANGE: &str = "Range";
/// `[Tooltip("How hard it hits")]`.
pub const TOOLTIP: &str = "Tooltip";
/// `[Header("Combat")]`.
pub const HEADER: &str = "Header";

/// The attributes a field takes.
pub const FIELD_ATTRIBUTES: [&str; 3] = [RANGE, TOOLTIP, HEADER];

/// What a field the inspector shows may be ranged as, after any `?` or `[]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Int,
    Float,
    Duration,
    Angle,
}

/// The hints `field`'s attributes declare. An attribute refused by
/// [`report_field_attributes`] is left out.
pub fn hints_of(field: &Field) -> FieldHints {
    let first = |name: &str| {
        field
            .attributes
            .iter()
            .find(|attribute| attribute.name == name)
    };
    FieldHints {
        range: first(RANGE).and_then(|attribute| {
            let kind = kind_of(&field.ty)?;
            let range = range_of(attribute, kind)?;
            not_empty(range).then_some(range)
        }),
        tooltip: first(TOOLTIP).and_then(text_of),
        header: first(HEADER).and_then(text_of),
    }
}

/// What is wrong with the attributes of `field`, one the inspector shows.
pub fn report_field_attributes(field: &Field, diagnostics: &mut Vec<Diagnostic>) {
    for (at, attribute) in field.attributes.iter().enumerate() {
        if !FIELD_ATTRIBUTES.contains(&attribute.name.as_str()) {
            diagnostics.push(
                Diagnostic::error(
                    format!("`[{}]` is not an attribute of a field", attribute.name),
                    attribute.span,
                )
                .with_note(
                    "a field takes `[Range(min, max)]`, `[Tooltip(\"…\")]` and `[Header(\"…\")]`",
                ),
            );
            continue;
        }
        if field.attributes[..at]
            .iter()
            .any(|earlier| earlier.name == attribute.name)
        {
            diagnostics.push(
                Diagnostic::error(
                    format!("`[{}]` is given twice", attribute.name),
                    attribute.span,
                )
                .with_note("a field takes each attribute once"),
            );
            continue;
        }
        match attribute.name.as_str() {
            RANGE => report_range(field, attribute, diagnostics),
            name => {
                if text_of(attribute).is_none() {
                    let example = if name == TOOLTIP {
                        "How hard it hits"
                    } else {
                        "Combat"
                    };
                    diagnostics.push(
                        Diagnostic::error(
                            format!("`[{name}(…)]` takes one text: `[{name}(\"{example}\")]`"),
                            attribute.span,
                        )
                        .with_note(if name == TOOLTIP {
                            "what hovering the field's label says, as a text literal that is not blank"
                        } else {
                            "the title shown above the field, as a text literal that is not blank"
                        }),
                    );
                }
            }
        }
    }
}

/// The attributes written on fields the inspector never shows: a struct's,
/// and a state's — in every module but the engine's.
pub fn report_misplaced(resolved: &Resolved) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let items = resolved
        .modules
        .iter()
        .filter(|module| module.path != crate::ENGINE_COMPONENTS_MODULE)
        .flat_map(|module| module.module.items.iter());
    for item in items {
        match item {
            Item::Struct(decl) => {
                for attribute in decl.fields.iter().filter_map(|f| f.attributes.first()) {
                    diagnostics.push(
                        Diagnostic::error("a struct field takes no attribute", attribute.span)
                            .with_note("only the fields of a component, a behavior or a system show in the inspector — put the attribute on the field holding the struct"),
                    );
                }
            }
            Item::Behavior(decl) | Item::System(decl) => {
                report_state_fields(decl, &mut diagnostics)
            }
            _ => {}
        }
    }
    diagnostics
}

/// The attributes written on the fields of `decl`'s states.
fn report_state_fields(decl: &BehaviorDecl, diagnostics: &mut Vec<Diagnostic>) {
    fn walk(members: &[BehaviorMember], in_state: bool, diagnostics: &mut Vec<Diagnostic>) {
        for member in members {
            match member {
                BehaviorMember::Field(field) if in_state => {
                    if let Some(attribute) = field.attributes.first() {
                        diagnostics.push(
                            Diagnostic::error("a state's field takes no attribute", attribute.span)
                                .with_note("a state's data is the engine's to keep, and shows read-only while playing — put the attribute on a field of the behavior"),
                        );
                    }
                }
                BehaviorMember::State(state) => walk(&state.members, true, diagnostics),
                _ => {}
            }
        }
    }
    walk(&decl.members, false, diagnostics);
}

fn report_range(field: &Field, attribute: &Attribute, diagnostics: &mut Vec<Diagnostic>) {
    let Some(kind) = kind_of(&field.ty) else {
        diagnostics.push(
            Diagnostic::error(
                "`[Range(…)]` applies to a number, a duration or an angle",
                attribute.span,
            )
            .with_note(format!(
                "`{}` is a `{}`: range an `int`, a `float`, a `Duration` or an `Angle`, or an optional or array of one",
                field.name,
                type_name(&field.ty)
            )),
        );
        return;
    };
    let Some(range) = range_of(attribute, kind) else {
        let (message, note) = match kind {
            Kind::Int => (
                "`[Range(…)]` on an `int` takes two integers: `[Range(0, 100)]`",
                "the lowest and the highest value editing it reaches, as integer literals",
            ),
            Kind::Float => (
                "`[Range(…)]` on a `float` takes two numbers: `[Range(0, 1)]`",
                "the lowest and the highest value editing it reaches, as number literals",
            ),
            Kind::Duration => (
                "`[Range(…)]` on a `Duration` takes two durations: `[Range(0s, 5s)]`",
                "the shortest and the longest editing it reaches, as duration literals — `250ms`, `2s`",
            ),
            Kind::Angle => (
                "`[Range(…)]` on an `Angle` takes two angles: `[Range(0deg, 90deg)]`",
                "the smallest and the largest editing it reaches, as angle literals — `90deg`, `1.5rad`",
            ),
        };
        diagnostics.push(Diagnostic::error(message, attribute.span).with_note(note));
        return;
    };
    if !not_empty(range) {
        diagnostics.push(
            Diagnostic::error(
                "this range is empty: its minimum must be below its maximum",
                attribute.span,
            )
            .with_note("both bounds finite, the first below the second"),
        );
        return;
    }
    report_default(field, attribute, kind, range, diagnostics);
}

/// A default outside `range`: a new row would start where editing cannot
/// bring it back.
fn report_default(
    field: &Field,
    attribute: &Attribute,
    kind: Kind,
    range: FieldRange,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let bounds = format!(
        "`[Range({}, {})]`",
        shown(kind, range.min),
        shown(kind, range.max)
    );
    let Some(default) = &field.default else {
        // With none, a number starts at zero; an optional at `null`, an
        // array empty — both inside any range.
        if matches!(field.ty, TypeRef::Named { .. }) && !range.contains(0.0) {
            diagnostics.push(
                Diagnostic::error(
                    format!("the default `{}` lies outside {bounds}", shown(kind, 0.0)),
                    attribute.span,
                )
                .with_note(format!(
                    "with no default, a new row starts at `{}`: give `{}` one inside the range, or widen it",
                    shown(kind, 0.0),
                    field.name
                )),
            );
        }
        return;
    };
    let mut values = Vec::new();
    if leaves(default, kind, &mut values).is_none() {
        return;
    }
    let inside = |value: f64| match kind {
        Kind::Int => range.contains(value),
        Kind::Float | Kind::Duration | Kind::Angle => range.holds(value),
    };
    let scalar = !matches!(default, Expr::ArrayLit { .. });
    if let Some(outside) = values.into_iter().find(|value| !inside(*value)) {
        let message = if scalar {
            format!(
                "the default `{}` lies outside {bounds}",
                shown(kind, outside)
            )
        } else {
            format!(
                "the default holds `{}`, outside {bounds}",
                shown(kind, outside)
            )
        };
        diagnostics.push(
            Diagnostic::error(message, attribute.span).with_note(
                "a new row starts at the default: give one inside the range, or widen it",
            ),
        );
    }
}

/// What `ty` may be ranged as: a number, a duration or an angle, or an
/// optional or array of one.
fn kind_of(ty: &TypeRef) -> Option<Kind> {
    match ty {
        TypeRef::Optional { inner, .. } => kind_of(inner),
        TypeRef::Array { element, .. } => kind_of(element),
        TypeRef::Named { name, .. } => match name.as_str() {
            "int" => Some(Kind::Int),
            "float" => Some(Kind::Float),
            "Duration" => Some(Kind::Duration),
            "Angle" => Some(Kind::Angle),
            _ => None,
        },
        _ => None,
    }
}

/// The two bounds `attribute` gives for a field of `kind`, in its stored
/// unit — or `None` unless it gives exactly two of that kind.
fn range_of(attribute: &Attribute, kind: Kind) -> Option<FieldRange> {
    match attribute.args.as_slice() {
        [min, max] => Some(FieldRange {
            min: bound(kind, min)?,
            max: bound(kind, max)?,
        }),
        _ => None,
    }
}

/// Whether `range` holds something: finite bounds, the first below the
/// second.
fn not_empty(range: FieldRange) -> bool {
    range.min.is_finite() && range.max.is_finite() && range.min < range.max
}

/// The value a literal of `kind` stands for, in its stored unit — seconds,
/// radians; negated included. A `float` takes integer literals as well.
fn bound(kind: Kind, expr: &Expr) -> Option<f64> {
    match (kind, expr) {
        (Kind::Int | Kind::Float, Expr::Int { value, .. }) => Some(*value as f64),
        (Kind::Float, Expr::Float { value, .. })
        | (Kind::Duration, Expr::Duration { value, .. })
        | (Kind::Angle, Expr::Angle { value, .. }) => Some(f64::from(*value)),
        (
            _,
            Expr::Unary {
                op: UnaryOp::Neg,
                operand,
                ..
            },
        ) if !matches!(**operand, Expr::Unary { .. }) => bound(kind, operand).map(|v| -v),
        _ => None,
    }
}

/// Gathers into `values` what a constant default holds, each of `kind` —
/// nothing for a `null`, every element of an array, at any depth; `None`
/// for a default that is not such a constant, which the type checker judges.
fn leaves(expr: &Expr, kind: Kind, values: &mut Vec<f64>) -> Option<()> {
    match expr {
        Expr::Null(_) => Some(()),
        Expr::ArrayLit { elements, .. } => elements
            .iter()
            .try_for_each(|element| leaves(element, kind, values)),
        other => {
            values.push(bound(kind, other)?);
            Some(())
        }
    }
}

/// `value` as the author writes a literal of `kind`.
fn shown(kind: Kind, value: f64) -> String {
    match kind {
        Kind::Int => format!("{}", value as i64),
        Kind::Float => format!("{}", value as f32),
        Kind::Duration => format!("{}s", value as f32),
        Kind::Angle => format!("{}deg", (value as f32).to_degrees()),
    }
}

/// The one text `attribute` gives, not blank.
fn text_of(attribute: &Attribute) -> Option<String> {
    match attribute.args.as_slice() {
        [Expr::Str { value, .. }] if !value.trim().is_empty() => Some(value.clone()),
        _ => None,
    }
}
