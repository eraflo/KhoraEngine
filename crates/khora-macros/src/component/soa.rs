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

//! The `Component` impl of a derived component, and its `SoaLayout` impl when
//! it opts into the field-SoA layout (`#[component(layout = "soa")]`): its
//! storage then goes through a `FieldSoaColumn<Self>`.

use quote::{quote, ToTokens};
use syn::{DeriveInput, Fields};

/// The `Component` impl carrying `script_methods`, and the `SoaLayout` impl —
/// empty unless `layout_soa`. `Err` holds what the derive emits instead when
/// the struct cannot take the SoA layout: the `Component` impl and a
/// `compile_error!`.
pub(super) fn component_impls(
    input: &DeriveInput,
    fields: &Fields,
    script_methods: &impl ToTokens,
    layout_soa: bool,
) -> Result<(impl ToTokens, impl ToTokens), impl ToTokens> {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let component_impl = quote! {
        impl #impl_generics crate::ecs::component::Component for #name #ty_generics #where_clause {
            #script_methods
        }
    };

    if !layout_soa {
        return Ok((component_impl, quote! {}));
    }

    let named = match fields {
        Fields::Named(n) => n,
        _ => {
            return Err(quote! {
                #component_impl
                compile_error!("#[component(layout = \"soa\")] requires a struct with named fields");
            });
        }
    };
    let mut field_idents = Vec::new();
    for f in &named.named {
        let is_f32 = matches!(&f.ty, syn::Type::Path(p) if p.path.is_ident("f32"));
        if !is_f32 {
            return Err(quote! {
                #component_impl
                compile_error!("#[component(layout = \"soa\")] (v1) supports only `f32` fields");
            });
        }
        field_idents.push(f.ident.clone().unwrap());
    }
    let field_count = field_idents.len();
    let field_names: Vec<String> = field_idents.iter().map(|i| i.to_string()).collect();
    let field_idx: Vec<usize> = (0..field_count).collect();

    let soa_layout = quote! {
        impl #impl_generics crate::ecs::soa::SoaLayout for #name #ty_generics #where_clause {
            const FIELD_COUNT: usize = #field_count;
            const FIELD_NAMES: &'static [&'static str] = &[#(#field_names),*];
            fn scatter_push(&self, fields: &mut [Vec<f32>]) {
                #(fields[#field_idx].push(self.#field_idents);)*
            }
            fn scatter_set(&self, fields: &mut [Vec<f32>], row: usize) {
                #(fields[#field_idx][row] = self.#field_idents;)*
            }
            fn gather(fields: &[Vec<f32>], row: usize) -> Self {
                Self { #(#field_idents: fields[#field_idx][row]),* }
            }
        }
    };
    let comp_impl = quote! {
        impl #impl_generics crate::ecs::component::Component for #name #ty_generics #where_clause {
            #script_methods
            fn make_column() -> Box<dyn crate::ecs::page::AnyVec> {
                Box::new(crate::ecs::soa::FieldSoaColumn::<#name>::new())
            }
            fn push_into_column(self, column: &mut dyn crate::ecs::page::AnyVec) {
                column
                    .as_any_mut()
                    .downcast_mut::<crate::ecs::soa::FieldSoaColumn<#name>>()
                    .expect("SoA column type mismatch")
                    .push(self);
            }
            fn copy_row_between(
                src: &dyn crate::ecs::page::AnyVec,
                src_row: usize,
                dst: &mut dyn crate::ecs::page::AnyVec,
            ) {
                let value = src
                    .as_any()
                    .downcast_ref::<crate::ecs::soa::FieldSoaColumn<#name>>()
                    .expect("SoA column type mismatch")
                    .get(src_row);
                dst.as_any_mut()
                    .downcast_mut::<crate::ecs::soa::FieldSoaColumn<#name>>()
                    .expect("SoA column type mismatch")
                    .push(value);
            }
            fn clone_from_column(column: &dyn crate::ecs::page::AnyVec, row: usize) -> Self {
                column
                    .as_any()
                    .downcast_ref::<crate::ecs::soa::FieldSoaColumn<#name>>()
                    .expect("SoA column type mismatch")
                    .get(row)
            }
            fn set_in_column(self, column: &mut dyn crate::ecs::page::AnyVec, row: usize) {
                column
                    .as_any_mut()
                    .downcast_mut::<crate::ecs::soa::FieldSoaColumn<#name>>()
                    .expect("SoA column type mismatch")
                    .set(row, self);
            }
        }
    };
    Ok((comp_impl, soa_layout))
}
