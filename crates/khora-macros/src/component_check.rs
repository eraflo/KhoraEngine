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
//! cannot use.

use quote::{quote, ToTokens};

/// The declared check's path, if any. Read in a pass of its own whose errors
/// are kept: a check that silently vanished would leave its values
/// unchecked.
pub(crate) fn check_path(input: &syn::DeriveInput) -> syn::Result<Option<syn::Path>> {
    let mut check = None;
    for attr in &input.attrs {
        if !attr.path().is_ident("component") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("check") {
                check = Some(meta.value()?.parse::<syn::Path>()?);
            } else if meta.input.peek(syn::Token![=]) {
                let _ = meta.value()?.parse::<syn::Expr>()?;
            }
            Ok(())
        })?;
    }
    Ok(check)
}

/// What the declared check adds to the registration of `name`: in `stage`,
/// an `Unusable` entry per field a loaded `value` cannot be used with (the
/// value itself is kept), and the `check` hook the running world's checks
/// call. Without a check, nothing and `None`.
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
