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

//! This crate provides procedural macros for the Khora Engine.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, Data, DeriveInput, Fields};

/// A derive macro that implements the `khora_data::ecs::Component` trait
/// and generates a serializable mirror struct with `From` conversions.
///
/// For a component like:
/// ```ignore
/// #[derive(Component)]
/// pub struct Camera {
///     pub projection: ProjectionType,
///     pub aspect_ratio: f32,
/// }
/// ```
///
/// This macro generates:
/// - `impl Component for Camera`
/// - `pub struct SerializableCamera { ... }` with `Encode, Decode`
/// - `impl From<Camera> for SerializableCamera`
/// - `impl From<SerializableCamera> for Camera`
///
/// Use `#[component(skip)]` on fields that should not be serialized
/// (e.g., GPU handles). Those fields are filled with `Default::default()`
/// when deserializing back.
#[proc_macro_derive(Component, attributes(component))]
pub fn derive_component(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let vis = &input.vis;
    let serializable_name = format_ident!("Serializable{}", name);
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    // Generate Component impl
    let component_impl = quote! {
        impl #impl_generics crate::ecs::component::Component for #name #ty_generics #where_clause {}
    };

    // Parse struct fields
    let fields = match &input.data {
        Data::Struct(data) => &data.fields,
        _ => {
            return TokenStream::from(quote! {
                #component_impl
                compile_error!("Component derive only supports structs");
            });
        }
    };

    // Check for #[component(no_serializable)] attribute
    let no_serializable = input.attrs.iter().any(|attr| {
        if !attr.path().is_ident("component") {
            return false;
        }
        let mut no = false;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("no_serializable") {
                no = true;
            }
            Ok(())
        });
        no
    });

    if no_serializable {
        return TokenStream::from(component_impl);
    }

    // Separate included and skipped fields
    let mut included_fields = Vec::new();
    let mut skipped_fields = Vec::new();

    for field in fields.iter() {
        let is_skip = field.attrs.iter().any(|attr| {
            if !attr.path().is_ident("component") {
                return false;
            }
            let mut skip = false;
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("skip") {
                    skip = true;
                }
                Ok(())
            });
            skip
        });

        if is_skip {
            skipped_fields.push(field);
        } else {
            included_fields.push(field);
        }
    }

    // Generate Serializable struct fields (only included fields)
    let serializable_field_defs: Vec<_> = included_fields
        .iter()
        .map(|f| {
            let fvis = &f.vis;
            let fname = &f.ident;
            let ftype = &f.ty;
            if let Some(fname) = fname {
                quote! { #fvis #fname: #ftype }
            } else {
                quote! { #fvis #ftype }
            }
        })
        .collect();

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

    let from_serializable_skipped: Vec<_> = skipped_fields
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

    // Determine struct kind for Serializable
    let serializable_struct = match fields {
        Fields::Named(_) if serializable_field_defs.is_empty() => {
            // All fields skipped → unit struct
            quote! {
                #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, bincode::Encode, bincode::Decode)]
                #vis struct #serializable_name;
            }
        }
        Fields::Named(_) => {
            quote! {
                #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, bincode::Encode, bincode::Decode)]
                #vis struct #serializable_name {
                    #(#serializable_field_defs),*
                }
            }
        }
        Fields::Unnamed(_) => {
            let field_types: Vec<_> = included_fields.iter().map(|f| &f.ty).collect();
            let field_vis: Vec<_> = included_fields.iter().map(|f| &f.vis).collect();
            quote! {
                #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, bincode::Encode, bincode::Decode)]
                #vis struct #serializable_name(#(#field_vis #field_types),*);
            }
        }
        Fields::Unit => {
            quote! {
                #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, bincode::Encode, bincode::Decode)]
                #vis struct #serializable_name;
            }
        }
    };

    // Generate From impls
    let from_original_to_serializable = if matches!(fields, Fields::Named(_))
        && !serializable_field_defs.is_empty()
    {
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
        let indices: Vec<syn::Index> = (0..included_fields.len()).map(syn::Index::from).collect();
        quote! {
            impl From<#name> for #serializable_name {
                fn from(value: #name) -> Self {
                    Self(#(value.#indices),*)
                }
            }
        }
    } else {
        quote! {}
    };

    let from_serializable_to_original = if matches!(fields, Fields::Named(_))
        && !serializable_field_defs.is_empty()
    {
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
        let indices: Vec<syn::Index> = (0..included_fields.len()).map(syn::Index::from).collect();
        quote! {
            impl From<#serializable_name> for #name {
                fn from(serializable: #serializable_name) -> Self {
                    Self(#(serializable.#indices),*)
                }
            }
        }
    } else {
        quote! {}
    };

    let expanded = quote! {
        #component_impl
        #serializable_struct
        #from_original_to_serializable
        #from_serializable_to_original

        // Auto-register this component for scene serialization.
        inventory::submit! {
            crate::scene::ComponentRegistration {
                type_id: std::any::TypeId::of::<#name>(),
                type_name: stringify!(#name),
                serialize_recipe: |world, entity| {
                    world.get::<#name>(entity).map(|c| {
                        bincode::encode_to_vec(&<#serializable_name>::from(c.clone()), bincode::config::standard())
                            .unwrap_or_default()
                    })
                },
                deserialize_recipe: |world, entity, data| {
                    let (s, _): (#serializable_name, _) = bincode::decode_from_slice_with_context(
                        data, bincode::config::standard(), ()
                    ).map_err(|e| e.to_string())?;
                    world.add_component(entity, <#name>::from(s)).ok();
                    Ok(())
                },
                create_default: |world, entity| {
                    world.add_component(entity, <#name>::default()).ok();
                    Ok(())
                },
            }
        }
    };

    TokenStream::from(expanded)
}
