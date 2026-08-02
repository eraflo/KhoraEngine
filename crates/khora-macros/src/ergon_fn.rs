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

//! `#[ergon_fn]` — exposing a Rust function to scripts.
//!
//! The macro writes three things next to the function: the signature the type
//! checker reads, the trampoline that unpacks a call, and the `inventory`
//! submission that puts it in the registry. Which is the point — a function
//! becomes callable from a script by being annotated, not by being annotated
//! *and* listed somewhere else that can be forgotten.
//!
//! # It does not read the type names
//!
//! Every parameter is translated through `ScriptType`, so `f32`, a type alias
//! for `f32`, and `crate::math::Scalar` all work, and a crate can expose its own
//! type by implementing that trait rather than by the macro learning about it.
//! Matching the written tokens would fail on all three.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, FnArg, ItemFn, Pat, ReturnType, Type};

/// What the attribute was given.
struct Options {
    /// The name scripts call it by.
    name: Option<String>,
    /// What one call costs against the frame's fuel.
    cost: Option<u64>,
}

pub fn ergon_fn(attr: TokenStream, item: TokenStream) -> TokenStream {
    let options = match parse_options(attr) {
        Ok(options) => options,
        Err(error) => return error.to_compile_error().into(),
    };
    let function = parse_macro_input!(item as ItemFn);

    match expand(&function, &options) {
        Ok(tokens) => tokens,
        Err(error) => error.to_compile_error().into(),
    }
}

fn parse_options(attr: TokenStream) -> syn::Result<Options> {
    let mut options = Options {
        name: None,
        cost: None,
    };
    if attr.is_empty() {
        return Ok(options);
    }

    let parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("name") {
            options.name = Some(meta.value()?.parse::<syn::LitStr>()?.value());
            Ok(())
        } else if meta.path.is_ident("cost") {
            options.cost = Some(meta.value()?.parse::<syn::LitInt>()?.base10_parse()?);
            Ok(())
        } else {
            Err(meta.error("expected `name = \"…\"` or `cost = …`"))
        }
    });
    syn::parse::Parser::parse(parser, attr)?;
    Ok(options)
}

fn expand(function: &ItemFn, options: &Options) -> syn::Result<TokenStream> {
    let ident = &function.sig.ident;
    let script_name = options
        .name
        .clone()
        .unwrap_or_else(|| pascal_case(&ident.to_string()));
    let cost = options.cost.unwrap_or(1);

    if let Some(token) = &function.sig.asyncness {
        return Err(syn::Error::new_spanned(
            token,
            "an engine function cannot be `async` — a script suspends through \
             `await`, which the VM drives; a Rust future has no way to be \
             resumed by the frame budget",
        ));
    }

    let (wants_context, params) = split_params(function)?;
    let param_types: Vec<&Type> = params.iter().map(|(_, ty)| *ty).collect();
    let bindings: Vec<_> = params
        .iter()
        .enumerate()
        .map(|(index, (_, ty))| {
            let slot = format_ident!("arg{index}");
            quote! {
                let #slot = <#ty as ::khora_script::native::ScriptType>::from_value(
                    args.get(#index).copied().unwrap_or(::khora_script::vm::Value::Unit),
                    context,
                )?;
            }
        })
        .collect();
    let slots: Vec<_> = (0..params.len()).map(|i| format_ident!("arg{i}")).collect();

    // A native that can refuse says so by returning `Result`, and the generated
    // trampoline propagates it — which is what lets one be written with `?`
    // instead of by hand.
    //
    // Recognised by the name of the return type, which is the one place this
    // macro does look at a written token. It is safe here in a way it would not
    // be for a parameter: a wrong guess produces a compile error on the very
    // next line, because the generated call would not type-check.
    let (result_ty, produced) = match &function.sig.output {
        ReturnType::Default => (quote!(()), quote!(let produced = ())),
        ReturnType::Type(_, ty) => match unwrap_result(ty) {
            Some(ok) => (quote!(#ok), quote!(let produced = __outcome?)),
            None => (quote!(#ty), quote!(let produced = __outcome)),
        },
    };

    let forwarded = if wants_context {
        quote!(context, #(#slots),*)
    } else {
        quote!(#(#slots),*)
    };

    let declaration = format_ident!("ERGON_{}", ident.to_string().to_uppercase());

    Ok(quote! {
        #function

        #[doc = concat!("Ergon binding for [`", stringify!(#ident), "`].")]
        #[allow(non_upper_case_globals)]
        pub static #declaration: ::khora_script::native::NativeFn =
            ::khora_script::native::NativeFn {
                name: #script_name,
                params: &[
                    #(<#param_types as ::khora_script::native::ScriptType>::TY),*
                ],
                result: <#result_ty as ::khora_script::native::ScriptType>::TY,
                cost: #cost,
                call: |context, args| {
                    #(#bindings)*
                    let __outcome = #ident(#forwarded);
                    #produced;
                    ::khora_script::native::ScriptType::to_value(produced, context)
                },
            };

        // Through `khora_script`'s re-export rather than `::inventory`, so a
        // crate exposing a function to Ergon needs the one dependency it already
        // has. Naming the collector crate directly would make `#[ergon_fn]` fail
        // to compile for the exact reason nobody would guess: a transitive
        // dependency that has to be declared again to be nameable.
        ::khora_script::inventory::submit! {
            ::khora_script::native::NativeRegistration(&#declaration)
        }
    }
    .into())
}

/// Splits off a leading `&mut NativeContext` parameter, if there is one.
///
/// Positional rather than marked with an attribute: it is the only borrowed
/// parameter a native can take, so there is nothing to disambiguate, and the
/// signature reads the way the call does.
type Params<'a> = Vec<(&'a Pat, &'a Type)>;

fn split_params(function: &ItemFn) -> syn::Result<(bool, Params<'_>)> {
    let mut wants_context = false;
    let mut params = Vec::new();

    for (index, input) in function.sig.inputs.iter().enumerate() {
        let FnArg::Typed(typed) = input else {
            return Err(syn::Error::new_spanned(
                input,
                "an engine function is a free function — `self` has no meaning \
                 to a script",
            ));
        };

        if index == 0 && is_context(&typed.ty) {
            wants_context = true;
            continue;
        }
        if is_context(&typed.ty) {
            return Err(syn::Error::new_spanned(
                &typed.ty,
                "the context comes first, or not at all",
            ));
        }
        params.push((&*typed.pat, &*typed.ty));
    }

    Ok((wants_context, params))
}

/// The `T` of a `Result<T, _>` return type, if that is what it is.
fn unwrap_result(ty: &Type) -> Option<&Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    let segment = path.path.segments.last()?;
    if segment.ident != "Result" {
        return None;
    }

    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    arguments.args.first().and_then(|argument| match argument {
        syn::GenericArgument::Type(inner) => Some(inner),
        _ => None,
    })
}

/// Whether a parameter is the native context.
///
/// By shape — a mutable reference to something named `NativeContext` — because
/// that is all a macro can see. Getting it wrong is caught by the compiler
/// immediately after, since the generated call would not type-check.
fn is_context(ty: &Type) -> bool {
    let Type::Reference(reference) = ty else {
        return false;
    };
    if reference.mutability.is_none() {
        return false;
    }
    let Type::Path(path) = &*reference.elem else {
        return false;
    };
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "NativeContext")
}

/// `apply_damage` → `ApplyDamage`.
///
/// Rust and Ergon disagree about how a function name looks, and the author
/// should not have to write both. An explicit `name = "…"` wins when the
/// mechanical answer is wrong.
fn pascal_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut capitalise = true;
    for character in name.chars() {
        if character == '_' {
            capitalise = true;
            continue;
        }
        if capitalise {
            out.extend(character.to_uppercase());
            capitalise = false;
        } else {
            out.push(character);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::pascal_case;

    #[test]
    fn a_snake_case_name_becomes_pascal_case() {
        assert_eq!(pascal_case("despawn"), "Despawn");
        assert_eq!(pascal_case("apply_damage"), "ApplyDamage");
        assert_eq!(pascal_case("raycast_all"), "RaycastAll");
    }

    #[test]
    fn an_already_capitalised_name_is_left_alone() {
        assert_eq!(pascal_case("Lerp"), "Lerp");
    }
}
