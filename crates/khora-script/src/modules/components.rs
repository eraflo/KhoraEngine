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

//! The components a project's scripts declare: what the compiler refuses
//! about them, and what it tells the engine.
//!
//! Checked per module, before the modules are merged, because two of the
//! rules are about *where* a declaration is written: the engine's own module
//! (`ENGINE_COMPONENTS_MODULE`) mirrors components the engine already stores,
//! and is held to neither.

use khora_core::ecs::SemanticDomain;
use khora_core::script::{ErgonType, ScriptValue};

use super::{Resolved, ResolvedModule};
use crate::arena::Owned;
use crate::ast::{Attribute, Expr, Field, Item, StructDecl, TypeRef};
use crate::bytecode::keys::type_name;
use crate::diagnostics::Diagnostic;
use crate::vm::{DeclaredComponent, DeclaredField, DeclaredKind, Program};

/// The attribute that places a component's data.
pub(super) const DOMAIN: &str = "Domain";

/// Every refusal about the components the project's own modules declare:
/// a name the engine uses, an attribute other than one `[Domain(…)]`, a
/// field the engine cannot store, a field with no zero and no default.
pub fn report_script_components(resolved: &Resolved, engine: &[String]) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for decl in script_components(resolved).map(|(_, decl)| decl) {
        if engine.contains(&decl.name) {
            diagnostics.push(
                Diagnostic::error(
                    format!(
                        "`{}` is an engine component. Choose another name, or `import \"{}\";` to use the engine's.",
                        decl.name,
                        crate::ENGINE_COMPONENTS_MODULE
                    ),
                    decl.name_span,
                )
                .with_note("a component's name is unique among every component the engine stores"),
            );
        }
        report_attributes(&decl.attributes, &mut diagnostics);
        for (at, field) in decl.fields.iter().enumerate() {
            if decl.fields[..at]
                .iter()
                .any(|earlier| earlier.name == field.name)
            {
                diagnostics.push(
                    Diagnostic::error(
                        format!("`{}` declares the field `{}` twice", decl.name, field.name),
                        field.span,
                    )
                    .with_note("a row holds one value per field name: rename or remove one"),
                );
            }
            report_field(field, &mut diagnostics);
            super::inspector::report_field_attributes(field, &mut diagnostics);
        }
    }
    diagnostics
}

/// The components the project's own modules declare, as the engine
/// registers them, sorted by name. `program` is what those modules compiled
/// to: its struct layouts hold each field's default.
pub fn declared_components(resolved: &Resolved, program: &Program) -> Vec<DeclaredComponent> {
    let mut declared: Vec<DeclaredComponent> = script_components(resolved)
        .map(|(module, decl)| DeclaredComponent {
            name: decl.name.clone(),
            module: module.path.clone(),
            domain: domain_of(&decl.attributes).unwrap_or(SemanticDomain::Script),
            fields: decl
                .fields
                .iter()
                .enumerate()
                .map(|(slot, field)| DeclaredField {
                    name: field.name.clone(),
                    ty: ergon_type(&field.ty).map_or_else(
                        || ErgonType::named_struct(type_name(&field.ty)),
                        |ty| with_struct_fields(ty, program, &mut Vec::new()),
                    ),
                    default: default_of(program, &decl.name, slot, &field.ty),
                    hints: super::inspector::hints_of(field),
                })
                .collect(),
            kind: DeclaredKind::Component,
            order: 0,
        })
        .collect();
    declared.sort_by(|a, b| a.name.cmp(&b.name));
    declared
}

/// Every `component` written outside the engine's module, with its module.
fn script_components(resolved: &Resolved) -> impl Iterator<Item = (&ResolvedModule, &StructDecl)> {
    resolved
        .modules
        .iter()
        .filter(|module| module.path != crate::ENGINE_COMPONENTS_MODULE)
        .flat_map(|module| {
            module
                .module
                .items
                .iter()
                .filter_map(move |item| match item {
                    Item::Component(decl) => Some((module, decl)),
                    _ => None,
                })
        })
}

/// One `[Domain(X)]` at most, naming a domain; nothing else.
fn report_attributes(attributes: &[Attribute], diagnostics: &mut Vec<Diagnostic>) {
    let mut placed = false;
    for attribute in attributes {
        if attribute.name != DOMAIN {
            diagnostics.push(
                Diagnostic::error(
                    format!("`[{}]` does not apply to a component", attribute.name),
                    attribute.span,
                )
                .with_note("a component takes one attribute, `[Domain(…)]`, which places its data; `[Critical]`, `[Budget(…)]` and `[Rate(…)]` schedule behaviors"),
            );
            continue;
        }
        if placed {
            diagnostics.push(
                Diagnostic::error(
                    "a component lives in one domain: `[Domain]` is given twice",
                    attribute.span,
                )
                .with_note("keep the one that says where its data belongs"),
            );
            continue;
        }
        placed = true;
        match attribute.args.as_slice() {
            [Expr::Ident { name, span }] => {
                if SemanticDomain::from_name(name).is_none() {
                    diagnostics.push(
                        Diagnostic::error(format!("`{name}` is not a domain"), *span)
                            .with_note(format!("`[Domain(…)]` names one of {}", known_domains())),
                    );
                }
            }
            _ => diagnostics.push(
                Diagnostic::error("`[Domain(…)]` takes one domain's name", attribute.span)
                    .with_note(format!(
                        "write `[Domain(Physics)]` — one of {}",
                        known_domains()
                    )),
            ),
        }
    }
}

/// The domain names, as an author writes them.
pub(super) fn known_domains() -> String {
    SemanticDomain::ALL
        .iter()
        .map(|domain| format!("`{}`", domain.name()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The domain `[Domain(X)]` names, when it names one.
pub(super) fn domain_of(attributes: &[Attribute]) -> Option<SemanticDomain> {
    attributes
        .iter()
        .filter(|attribute| attribute.name == DOMAIN)
        .find_map(|attribute| match attribute.args.as_slice() {
            [Expr::Ident { name, .. }] => SemanticDomain::from_name(name),
            _ => None,
        })
}

/// A field the engine cannot store, or that has nothing to start a row at.
fn report_field(field: &Field, diagnostics: &mut Vec<Diagnostic>) {
    if ergon_type(&field.ty).is_none() {
        diagnostics.push(
            Diagnostic::error(
                format!(
                    "a component field cannot hold `{}`",
                    type_name(&field.ty)
                ),
                field.ty.span(),
            )
            .with_note("a component holds numbers, flags, text, entities, engine values, structs, and optionals and arrays of those"),
        );
        return;
    }
    if let Some(span) = field.default.as_ref().and_then(non_finite) {
        diagnostics.push(
            Diagnostic::error("this number is beyond what a float holds", span)
                .with_note("a float holds up to about 3.4e38: a larger literal reads as infinity, which no field stores"),
        );
    }
    if field.default.is_none() && Owned::zero(&type_name(&field.ty)).is_none() {
        diagnostics.push(
            Diagnostic::error(
                format!(
                    "`{}` needs a default: a `{}` has no zero to start a new row at",
                    field.name,
                    type_name(&field.ty)
                ),
                field.span,
            )
            .with_note(
                match khora_core::asset::AssetKind::from_ergon_name(&type_name(&field.ty)) {
                    Some(kind) => crate::types::ty::asset_field_note(kind),
                    None => "write `= …`, or make the field optional (`T?`) so a new row starts at `null`".to_owned(),
                },
            ),
        );
    }
}

/// Where in `expr`, a default, a float literal stands for no finite number.
pub(super) fn non_finite(expr: &Expr) -> Option<crate::diagnostics::Span> {
    match expr {
        Expr::Float { value, span }
        | Expr::Duration { value, span }
        | Expr::Angle { value, span } => (!value.is_finite()).then_some(*span),
        Expr::Unary { operand, .. } => non_finite(operand),
        Expr::ArrayLit { elements, .. } => elements.iter().find_map(non_finite),
        Expr::StructLit { fields, .. } => fields.iter().find_map(|(_, _, value)| non_finite(value)),
        Expr::Call { args, .. } => args.iter().find_map(non_finite),
        _ => None,
    }
}

/// What a field type is to the engine's storage. `None` for one it cannot
/// store (`Map<…>`).
pub(crate) fn ergon_type(ty: &TypeRef) -> Option<ErgonType> {
    Some(match ty {
        TypeRef::Void | TypeRef::Generic { .. } => return None,
        TypeRef::Optional { inner, .. } => ErgonType::Optional(Box::new(ergon_type(inner)?)),
        TypeRef::Array { element, .. } => ErgonType::Array(Box::new(ergon_type(element)?)),
        // A duration and an angle keep their unit: stored as floats, they
        // are typed so — what a tool shows them in.
        TypeRef::Named { name, .. } => match name.as_str() {
            "void" => return None,
            other => ErgonType::parse(other)?,
        },
    })
}

/// `ty` with every struct it names given its fields, from the program's
/// layouts: a struct changed between two versions of a component is then a
/// field of another type. A struct already being expanded — one that holds
/// itself — stays known by its name.
pub(super) fn with_struct_fields(
    ty: ErgonType,
    program: &Program,
    expanding: &mut Vec<String>,
) -> ErgonType {
    match ty {
        ErgonType::Optional(inner) => {
            ErgonType::Optional(Box::new(with_struct_fields(*inner, program, expanding)))
        }
        ErgonType::Array(element) => {
            ErgonType::Array(Box::new(with_struct_fields(*element, program, expanding)))
        }
        ErgonType::Struct { name, fields: None } if !expanding.contains(&name) => {
            let Some((_, layout)) = program.struct_named(&name) else {
                return ErgonType::Struct { name, fields: None };
            };
            expanding.push(name.clone());
            let fields = layout
                .fields
                .iter()
                .map(|(field, written)| {
                    let ty = ErgonType::parse(written)
                        .unwrap_or_else(|| ErgonType::named_struct(written.clone()));
                    (field.clone(), with_struct_fields(ty, program, expanding))
                })
                .collect();
            expanding.pop();
            ErgonType::Struct {
                name,
                fields: Some(fields),
            }
        }
        other => other,
    }
}

/// A field's default as the engine stores it: the declared one, completed,
/// else the type's zero.
fn default_of(program: &Program, component: &str, slot: usize, ty: &TypeRef) -> ScriptValue {
    let declared = program
        .struct_named(component)
        .and_then(|(_, layout)| layout.defaults.get(slot).cloned().flatten());
    let value = declared
        .or_else(|| Owned::zero(&type_name(ty)))
        .and_then(|owned| crate::bridge::from_owned(&owned).ok())
        .unwrap_or(ScriptValue::Null);
    match ergon_type(ty) {
        Some(ty) => ty.widened(value),
        None => value,
    }
}
