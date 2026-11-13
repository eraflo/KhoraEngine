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

//! The `Serializable<Name>` mirror of a derived component — the struct serde
//! reads and writes for saves and the inspector — and the two `From` impls
//! between the component and its mirror.

use quote::{quote, ToTokens};
use syn::{DeriveInput, Fields};

use super::fields::FieldSplit;

/// The mirror struct of `input`, named `serializable_name`, and its `From`
/// impls in both directions.
pub(super) fn mirror(
    input: &DeriveInput,
    serializable_name: &syn::Ident,
    fields: &Fields,
    split: &FieldSplit,
) -> impl ToTokens {
    let name = &input.ident;

    // Generate Serializable struct fields (only included fields)
    let serializable_field_defs: Vec<_> = split
        .included
        .iter()
        .zip(&split.formerly)
        .map(|(f, former)| {
            let fvis = &f.vis;
            let fname = &f.ident;
            let ftype = &f.ty;
            if let Some(fname) = fname {
                quote! { #(#[serde(alias = #former)])* #fvis #fname: #ftype }
            } else {
                quote! { #fvis #ftype }
            }
        })
        .collect();

    let serializable_struct = mirror_struct(
        input,
        serializable_name,
        fields,
        &split.included,
        &serializable_field_defs,
    );
    let named_with_fields =
        matches!(fields, Fields::Named(_)) && !serializable_field_defs.is_empty();
    let from_original_to_serializable = into_mirror(
        name,
        serializable_name,
        fields,
        &split.included,
        named_with_fields,
    );
    let from_serializable_to_original =
        from_mirror(name, serializable_name, fields, split, named_with_fields);

    quote! {
        #serializable_struct
        #from_original_to_serializable
        #from_serializable_to_original
    }
}

/// The mirror struct itself.
///
/// The `Serializable<Name>` mirror is a generated implementation detail
/// used by the scene serialization pipeline and the editor inspector.
/// It is `pub` because callers of `#[derive(Component)]` can be public,
/// but it is not part of the documented public API surface. We tag it
/// with `#[allow(missing_docs)]` so that crates with `#![warn(missing_docs)]`
/// do not require users to document every mirror field by hand.
fn mirror_struct(
    input: &DeriveInput,
    serializable_name: &syn::Ident,
    fields: &Fields,
    included_fields: &[&syn::Field],
    serializable_field_defs: &[impl ToTokens],
) -> impl ToTokens {
    let name = &input.ident;
    let vis = &input.vis;
    // Determine struct kind for Serializable.
    match fields {
        Fields::Named(_) if serializable_field_defs.is_empty() => {
            // All fields skipped → unit struct
            quote! {
                #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
                #[allow(missing_docs)]
                #[doc(hidden)]
                #vis struct #serializable_name;
            }
        }
        // `#[serde(default)]`: a field a save predates takes the value a
        // fresh component has, which is what the author's type says it is.
        Fields::Named(_) => {
            quote! {
                #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
                #[serde(default)]
                #[allow(missing_docs)]
                #[doc(hidden)]
                #vis struct #serializable_name {
                    #(#serializable_field_defs),*
                }

                impl Default for #serializable_name {
                    fn default() -> Self {
                        <#name as Default>::default().into()
                    }
                }
            }
        }
        Fields::Unnamed(_) => {
            let field_types: Vec<_> = included_fields.iter().map(|f| &f.ty).collect();
            let field_vis: Vec<_> = included_fields.iter().map(|f| &f.vis).collect();
            quote! {
                #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
                #[allow(missing_docs)]
                #[doc(hidden)]
                #vis struct #serializable_name(#(#field_vis #field_types),*);
            }
        }
        Fields::Unit => {
            quote! {
                #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
                #[allow(missing_docs)]
                #[doc(hidden)]
                #vis struct #serializable_name;
            }
        }
    }
}

/// `From<Name> for Serializable<Name>`. `named_with_fields`: a struct with
/// named fields of which the mirror holds at least one.
fn into_mirror(
    name: &syn::Ident,
    serializable_name: &syn::Ident,
    fields: &Fields,
    included_fields: &[&syn::Field],
    named_with_fields: bool,
) -> impl ToTokens {
    // Generate field assignments for From<Original> → Serializable
    let to_serializable_assigns: Vec<_> = included_fields
        .iter()
        .map(|f| {
            let fname = &f.ident;
            if fname.is_some() {
                quote! { #fname: value.#fname }
            } else {
                quote! { value.#fname }
            }
        })
        .collect();

    if named_with_fields {
        quote! {
            impl From<#name> for #serializable_name {
                fn from(value: #name) -> Self {
                    Self {
                        #(#to_serializable_assigns),*
                    }
                }
            }
        }
    } else if matches!(fields, Fields::Unnamed(_)) {
        // The mirror holds the included fields in order; each is read from its
        // real position in the original, past any skipped field before it.
        let positions: Vec<syn::Index> = fields
            .iter()
            .enumerate()
            .filter(|(_, field)| {
                included_fields
                    .iter()
                    .any(|kept| std::ptr::eq(*kept, *field))
            })
            .map(|(position, _)| syn::Index::from(position))
            .collect();
        quote! {
            impl From<#name> for #serializable_name {
                fn from(value: #name) -> Self {
                    Self(#(value.#positions),*)
                }
            }
        }
    } else {
        // A marker — a unit struct, or one whose every field is skipped. The
        // mirror above is generated for it; without these two impls it had a
        // type and no way to reach it, so the registration did not compile and
        // the only way to declare such a component was
        // `#[component(no_serializable)]` — which drops the registration
        // entirely and makes the marker vanish from every scene file. A marker
        // carries no data and all of its meaning: its presence *is* the value.
        quote! {
            impl From<#name> for #serializable_name {
                fn from(_: #name) -> Self {
                    Self
                }
            }
        }
    }
}

/// `From<Serializable<Name>> for Name`: included fields from the mirror,
/// skipped fields from their `Default`. `named_with_fields` as in
/// [`into_mirror`].
fn from_mirror(
    name: &syn::Ident,
    serializable_name: &syn::Ident,
    fields: &Fields,
    split: &FieldSplit,
    named_with_fields: bool,
) -> impl ToTokens {
    let included_fields = &split.included;

    // Generate field assignments for From<Serializable> → Original
    // Included fields come from the serializable, skipped fields use Default
    let from_serializable_included: Vec<_> = included_fields
        .iter()
        .map(|f| {
            let fname = &f.ident;
            if fname.is_some() {
                quote! { #fname: serializable.#fname }
            } else {
                quote! { serializable.#fname }
            }
        })
        .collect();

    let from_serializable_skipped: Vec<_> = split
        .skipped
        .iter()
        .map(|f| {
            let fname = &f.ident;
            if fname.is_some() {
                quote! { #fname: Default::default() }
            } else {
                quote! { Default::default() }
            }
        })
        .collect();

    let all_from_fields: Vec<_> = from_serializable_included
        .into_iter()
        .chain(from_serializable_skipped)
        .collect();

    if named_with_fields {
        quote! {
            impl From<#serializable_name> for #name {
                fn from(serializable: #serializable_name) -> Self {
                    Self {
                        #(#all_from_fields),*
                    }
                }
            }
        }
    } else if matches!(fields, Fields::Unnamed(_)) {
        // Every position of the original, in order: an included field from its
        // slot in the mirror, a skipped one from its `Default`.
        let mut slot = 0;
        let values: Vec<_> = fields
            .iter()
            .map(|field| {
                if included_fields
                    .iter()
                    .any(|kept| std::ptr::eq(*kept, field))
                {
                    let index = syn::Index::from(slot);
                    slot += 1;
                    quote! { serializable.#index }
                } else {
                    quote! { Default::default() }
                }
            })
            .collect();
        quote! {
            impl From<#serializable_name> for #name {
                fn from(serializable: #serializable_name) -> Self {
                    Self(#(#values),*)
                }
            }
        }
    } else {
        // The other direction for a marker. `Default` rather than `Self`,
        // because a struct whose fields were all skipped has fields to fill.
        quote! {
            impl From<#serializable_name> for #name {
                fn from(_: #serializable_name) -> Self {
                    Self::default()
                }
            }
        }
    }
}
