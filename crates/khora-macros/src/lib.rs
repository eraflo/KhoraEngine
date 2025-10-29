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
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

/// A derive macro that implements the `khora_data::ecs::Component` trait.
///
/// It also verifies that the struct meets the trait's supertrait bounds
/// (`Clone`, `Send`, `Sync`, `'static`).
#[proc_macro_derive(Component)]
pub fn derive_component(input: TokenStream) -> TokenStream {
    // Parse the input tokens into a syntax tree.
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    // Generate the implementation of the Component trait.
    // The `where` clause is crucial to ensure that the struct itself
    // meets the supertrait bounds required by `Component`.
    let expanded = quote! {
        impl #impl_generics crate::ecs::component::Component for #name #ty_generics #where_clause {}
    };

    // Hand the output tokens back to the compiler.
    TokenStream::from(expanded)
}
