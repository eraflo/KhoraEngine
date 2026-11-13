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

//! The field schema a derived component registers: the name and type of
//! each field its mirror holds.

use quote::{quote, ToTokens};
use syn::Fields;

/// The `ComponentShape` of a component whose mirror holds `included_fields`.
///
/// The field schema, built from the very list that feeds the `Serializable`
/// mirror. Sharing the list is the point: a field the mirror omits is a field
/// the schema must omit too, and deriving them separately would let
/// `#[component(skip)]` be honoured in one place and forgotten in the other.
pub(super) fn field_schema(fields: &Fields, included_fields: &[&syn::Field]) -> impl ToTokens {
    let entries: Vec<_> = match fields {
        Fields::Named(_) => included_fields
            .iter()
            .map(|f| {
                let name = f.ident.as_ref().expect("named field");
                let ty = &f.ty;
                quote! {
                    crate::scene::FieldSchema {
                        name: stringify!(#name),
                        ty: stringify!(#ty),
                    }
                }
            })
            .collect(),
        Fields::Unnamed(_) => included_fields
            .iter()
            .enumerate()
            .map(|(i, f)| {
                // A tuple field has no name; its index is what an inspector
                // shows and what a generator addresses it by.
                let index = i.to_string();
                let ty = &f.ty;
                quote! {
                    crate::scene::FieldSchema {
                        name: #index,
                        ty: stringify!(#ty),
                    }
                }
            })
            .collect(),
        // A marker carries no data, and saying so is the truthful answer.
        Fields::Unit => Vec::new(),
    };
    // Always `Fields`: this derive only accepts structs.
    quote! { crate::scene::ComponentShape::Fields(&[#(#entries),*]) }
}
