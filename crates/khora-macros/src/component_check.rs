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

//! `#[component(check = path::to::fn)]` — a component's own check, a
//! `fn(&Self) -> Vec<FieldProblem>` naming the fields whose values the engine
//! cannot use. The key itself is read with the type's other keys
//! (`crate::component::attrs`).

use quote::{quote, ToTokens};

/// Returns what the declared check adds to the registration of `name`: its stage step and its hook.
///
/// The stage step reports an `Unusable` entry per field a loaded `value`
/// cannot be used with, keeping the value; the running world's checks call
/// the `check` hook. Without a check, nothing and `None`.
pub(crate) fn check_tokens(
    name: &syn::Ident,
    check: Option<&syn::Path>,
) -> (impl ToTokens, impl ToTokens) {
    match check {
        Some(check) => (
            quote! {
                report.extend(crate::scene::record::unusable_entries(#check(&value)));
            },
            quote! {
                Some(|world| crate::ecs::check_all::<#name>(world, #check))
            },
        ),
        None => (quote! {}, quote! { None }),
    }
}
