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

//! `#[derive(LaneInputs)]` and `#[derive(LaneOutputs)]`: a lane's typed
//! inputs and its family's outputs.
//!
//! Every path the output names is absolute (`::khora_core::lane::…`), and its
//! own locals are hygienic, so no field name collides with them. Each
//! field goes through its attribute's check in `khora_core::lane::compile_check`,
//! spanned on the field's type: the check both rejects a field of the wrong
//! shape — once, on the field, with the attribute's message — and hands back
//! how to declare and fetch it. Every parse error is a `syn::Error` on the
//! offending span, returned before any `impl` is emitted.

use proc_macro::TokenStream;
use quote::{format_ident, quote, quote_spanned, ToTokens};
use syn::ext::IdentExt;
use syn::spanned::Spanned;
use syn::{
    parse_macro_input, parse_quote, Attribute, Data, DeriveInput, Field, Fields, GenericParam,
    Ident, Lifetime,
};

/// The input attributes, in the order the "names no source" message lists them.
const INPUT_SOURCES: &[&str] = &[
    "bus", "frame", "deck", "resident", "resource", "locked", "budget", "scope",
];

/// The output attributes.
const OUTPUT_SINKS: &[&str] = &["pass", "deck"];

/// The impl header's pieces: the struct's generics with the frame lifetime
/// `'f` in them, the lifetime itself, and the struct's own type generics.
fn frame_generics(input: &DeriveInput) -> (syn::Generics, Lifetime) {
    let mut generics = input.generics.clone();
    let frame = generics.lifetimes().next().map(|l| l.lifetime.clone());
    match frame {
        Some(lifetime) => (generics, lifetime),
        None => {
            let lifetime: Lifetime = parse_quote!('__khora_f);
            generics
                .params
                .insert(0, GenericParam::Lifetime(parse_quote!(#lifetime)));
            (generics, lifetime)
        }
    }
}

/// The struct's named fields, or the error saying why it has none.
fn named_fields<'a>(input: &'a DeriveInput, derive: &str) -> syn::Result<Vec<&'a Field>> {
    match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => Ok(fields.named.iter().collect()),
            Fields::Unit => Ok(Vec::new()),
            Fields::Unnamed(fields) => Err(syn::Error::new(
                fields.span(),
                format!("`{derive}` names each field: use a struct with named fields"),
            )),
        },
        _ => Err(syn::Error::new(
            input.ident.span(),
            format!("`{derive}` derives for a struct, one field per source"),
        )),
    }
}

/// One field and the attribute that says where it comes from or goes.
struct Annotated<'a> {
    ident: &'a Ident,
    ty: &'a syn::Type,
    attribute: String,
    validate: bool,
}

/// Reads the one attribute among `allowed` a field carries, and its argument.
fn annotate<'a>(
    field: &'a Field,
    allowed: &[&str],
    validating: &[&str],
) -> syn::Result<Annotated<'a>> {
    let ident = field
        .ident
        .as_ref()
        .ok_or_else(|| syn::Error::new(field.span(), "a lane field has a name"))?;
    let mut found: Option<(&Attribute, String)> = None;
    for attribute in &field.attrs {
        let Some(name) = attribute.path().get_ident().map(Ident::to_string) else {
            continue;
        };
        if !allowed.contains(&name.as_str()) {
            continue;
        }
        if let Some((_, first)) = &found {
            return Err(syn::Error::new(
                attribute.span(),
                format!(
                    "field `{ident}` names two sources, `#[{first}]` and `#[{name}]`: keep one"
                ),
            ));
        }
        found = Some((attribute, name));
    }
    let Some((attribute, name)) = found else {
        let listed = allowed
            .iter()
            .map(|name| format!("`#[{name}]`"))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(syn::Error::new(
            ident.span(),
            format!("field `{ident}` names no source: add one of {listed}"),
        ));
    };
    let mut validate = false;
    if let syn::Meta::NameValue(pair) = &attribute.meta {
        let argument = if validating.contains(&name.as_str()) {
            format!("`#[{name}]` takes its argument in parentheses: `#[{name}(validate)]`")
        } else {
            format!("`#[{name}]` takes no argument")
        };
        return Err(syn::Error::new(pair.value.span(), argument));
    }
    if let syn::Meta::List(list) = &attribute.meta {
        list.parse_nested_meta(|meta| {
            let argument = meta
                .path
                .get_ident()
                .map(Ident::to_string)
                .unwrap_or_default();
            if validating.contains(&name.as_str()) {
                if argument == "validate" {
                    validate = true;
                    return Ok(());
                }
                return Err(meta.error(format!(
                    "unknown `#[{name}]` argument `{argument}`: the only one is `validate`"
                )));
            }
            Err(meta.error(format!("`#[{name}]` takes no argument")))
        })?;
    }
    Ok(Annotated {
        ident,
        ty: &field.ty,
        attribute: name,
        validate,
    })
}

/// A local of the generated code, hygienic: no field of the struct, whatever
/// its name, can shadow it.
fn local(name: &str) -> Ident {
    Ident::new(name, proc_macro::Span::mixed_site().into())
}

/// The path of the check for `attribute`, called with the field's type and
/// spanned on it.
fn check(attribute: &str, ty: &syn::Type) -> impl ToTokens {
    let function = format_ident!("{}_field", attribute);
    quote_spanned!(ty.span()=> ::khora_core::lane::compile_check::#function::<#ty>())
}

/// Derives `::khora_core::lane::io::LaneInputs`.
pub fn derive_lane_inputs(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match lane_inputs(&input) {
        Ok(tokens) => tokens.into_token_stream().into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn lane_inputs(input: &DeriveInput) -> syn::Result<impl ToTokens> {
    let mut fields = Vec::new();
    for field in named_fields(input, "LaneInputs")? {
        let annotated = annotate(field, INPUT_SOURCES, &["bus", "deck"])?;
        if annotated.attribute == "scope" {
            return Err(syn::Error::new(
                annotated.ty.span(),
                "`#[scope]` gives a parallel scope, which no lane can take yet",
            ));
        }
        fields.push(annotated);
    }

    let name = &input.ident;
    let (generics, frame) = frame_generics(input);
    let (impl_generics, _, where_clause) = generics.split_for_impl();
    let (_, ty_generics, _) = input.generics.split_for_impl();

    let (d, src, value) = (local("d"), local("src"), local("value"));
    let mut declares = Vec::new();
    let mut fetches = Vec::new();
    let mut targets = Vec::new();
    for field in &fields {
        let ident = field.ident;
        let label = ident.unraw().to_string();
        let attribute = match field.attribute.as_str() {
            "deck" => "deck_input",
            other => other,
        };
        let ops = check(attribute, field.ty);
        declares.push(quote!((#ops.declare)(#d, #label);));
        fetches.push(quote!(let #ident = (#ops.fetch)(#src, #label)?;));
        if field.validate {
            let source = match field.attribute.as_str() {
                "bus" => quote!(::khora_core::lane::guard::SourceKind::Bus),
                _ => quote!(::khora_core::lane::guard::SourceKind::Deck),
            };
            fetches.push(match field.attribute.as_str() {
                "bus" => quote!(#src.validate(#label, #source, #ident);),
                _ => quote! {
                    if let ::core::option::Option::Some(#value) = #ident {
                        #src.validate(#label, #source, #value);
                    }
                },
            });
        }
        if field.attribute == "frame" {
            targets.push(quote!(.or_else(|| (#ops.target)(&self.#ident))));
        }
    }
    let idents = fields.iter().map(|field| field.ident);

    Ok(quote! {
        impl #impl_generics ::khora_core::lane::io::LaneInputs<#frame> for #name #ty_generics #where_clause {
            fn declare(#d: &mut ::khora_core::lane::io::Declaration) {
                #(#declares)*
            }
            fn fetch(
                #src: &mut ::khora_core::lane::io::InputSource<#frame>,
            ) -> ::core::result::Result<Self, ::khora_core::lane::guard::InputRefusal> {
                #(#fetches)*
                ::core::result::Result::Ok(Self { #(#idents),* })
            }
            fn target_key(&self) -> ::core::option::Option<::khora_core::lane::io::TargetKey> {
                ::core::option::Option::None #(#targets)*
            }
        }
    })
}

/// Derives `::khora_core::lane::io::LaneOutputs`.
pub fn derive_lane_outputs(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match lane_outputs(&input) {
        Ok(tokens) => tokens.into_token_stream().into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn lane_outputs(input: &DeriveInput) -> syn::Result<impl ToTokens> {
    let mut fields = Vec::new();
    for field in named_fields(input, "LaneOutputs")? {
        fields.push(annotate(field, OUTPUT_SINKS, &[])?);
    }

    let name = &input.ident;
    let (generics, frame) = frame_generics(input);
    let (impl_generics, _, where_clause) = generics.split_for_impl();
    let (_, ty_generics, _) = input.generics.split_for_impl();

    let (d, sink) = (local("d"), local("sink"));
    let mut declares = Vec::new();
    let mut opens = Vec::new();
    let mut commits = Vec::new();
    for field in &fields {
        let ident = field.ident;
        let label = ident.unraw().to_string();
        let attribute = match field.attribute.as_str() {
            "deck" => "deck_output",
            other => other,
        };
        let ops = check(attribute, field.ty);
        declares.push(quote!((#ops.declare)(#d, #label);));
        opens.push(quote!(let #ident = (#ops.open)(#sink, #label)?;));
        commits.push(quote!((#ops.commit)(#ident, #sink, #label)?;));
    }
    let idents: Vec<_> = fields.iter().map(|field| field.ident).collect();

    Ok(quote! {
        impl #impl_generics ::khora_core::lane::io::LaneOutputs<#frame> for #name #ty_generics #where_clause {
            fn declare(#d: &mut ::khora_core::lane::io::Declaration) {
                #(#declares)*
            }
            fn open(
                #sink: &mut ::khora_core::lane::io::OutputSink<#frame>,
            ) -> ::core::result::Result<Self, ::khora_core::lane::LaneError> {
                #(#opens)*
                ::core::result::Result::Ok(Self { #(#idents),* })
            }
            fn commit(
                self,
                #sink: &mut ::khora_core::lane::io::OutputSink<#frame>,
            ) -> ::core::result::Result<(), ::khora_core::lane::guard::OutputRefusal> {
                let Self { #(#idents),* } = self;
                #(#commits)*
                ::core::result::Result::Ok(())
            }
        }
    })
}
