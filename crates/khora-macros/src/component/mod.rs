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

//! `#[derive(Component)]`: the `Component` impl, the serializable mirror and
//! the registrations a component needs to be stored, saved and inspected.

mod attrs;
mod fields;
mod inventory;
mod mirror;
mod schema;
mod soa;

use proc_macro::TokenStream;
use quote::{format_ident, quote, ToTokens};
use syn::{parse_macro_input, Data, DeriveInput};

use crate::script_fields::script_methods;
use attrs::ComponentAttrs;
use fields::FieldSplit;

pub fn derive_component(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
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

    let split = FieldSplit::of(fields);
    let script_methods = script_methods(fields, &split.included);
    let attrs = ComponentAttrs::of(&input);

    let (component_impl, soa_layout_impl) =
        match soa::component_impls(&input, fields, &script_methods, attrs.layout_soa) {
            Ok(impls) => impls,
            Err(error) => return TokenStream::from(error.into_token_stream()),
        };

    let check_path = match crate::component_check::check_path(&input) {
        Ok(check) => check,
        Err(error) => return TokenStream::from(error.to_compile_error()),
    };

    let domain_registration = inventory::domain_registration(name, attrs.domain.as_ref());

    if attrs.no_serializable {
        return TokenStream::from(quote! {
            #component_impl
            #domain_registration
            #soa_layout_impl
        });
    }

    let field_schema = schema::field_schema(fields, &split.included);
    let mirror = mirror::mirror(&input, &serializable_name, fields, &split);

    let schema_cell = format_ident!("__KHORA_SCHEMA_OF_{}", name);
    let registration = inventory::component_registration(
        name,
        &serializable_name,
        &attrs,
        &field_schema,
        &schema_cell,
        check_path.as_ref(),
    );
    let expanded = quote! {
        #component_impl
        #domain_registration

        // The mirror's schema, traced once: it cannot change while the
        // program runs.
        #[doc(hidden)]
        #[allow(non_upper_case_globals)]
        static #schema_cell: ::std::sync::OnceLock<(u64, bool)> = ::std::sync::OnceLock::new();
        #soa_layout_impl
        #mirror
        #registration
    };

    TokenStream::from(expanded)
}
