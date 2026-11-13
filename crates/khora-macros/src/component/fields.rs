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

//! A derived component's fields, split into those the mirror saves and those
//! `#[component(skip)]` leaves out, with each saved field's former names.

use syn::Fields;

/// The fields of a component, as the mirror sees them.
pub(super) struct FieldSplit<'a> {
    /// The fields the mirror holds, in declaration order.
    pub(super) included: Vec<&'a syn::Field>,
    /// The fields tagged `#[component(skip)]`, in declaration order.
    pub(super) skipped: Vec<&'a syn::Field>,
    /// A field's former names, read by the mirror as serde aliases: a save
    /// written before the rename still fills the field (named fields only — a
    /// tuple field has no name to rename). One entry per included field.
    pub(super) formerly: Vec<Vec<syn::LitStr>>,
}

impl<'a> FieldSplit<'a> {
    /// Splits `fields` by their `#[component(...)]` field attributes.
    pub(super) fn of(fields: &'a Fields) -> Self {
        // Separate included and skipped fields
        let mut included_fields = Vec::new();
        let mut skipped_fields = Vec::new();

        let mut field_formerly: Vec<Vec<syn::LitStr>> = Vec::new();
        for field in fields.iter() {
            let mut is_skip = false;
            let mut former = Vec::new();
            for attr in &field.attrs {
                if !attr.path().is_ident("component") {
                    continue;
                }
                let _ = attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("skip") {
                        is_skip = true;
                    } else if meta.path.is_ident("formerly") {
                        former.push(meta.value()?.parse::<syn::LitStr>()?);
                    } else if meta.input.peek(syn::Token![=]) {
                        let _ = meta.value()?.parse::<syn::Expr>()?;
                    }
                    Ok(())
                });
            }

            if !is_skip {
                field_formerly.push(former);
            }
            if is_skip {
                skipped_fields.push(field);
            } else {
                included_fields.push(field);
            }
        }

        Self {
            included: included_fields,
            skipped: skipped_fields,
            formerly: field_formerly,
        }
    }
}
