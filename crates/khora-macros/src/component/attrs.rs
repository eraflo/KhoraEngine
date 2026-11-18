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

//! The type-level `#[component(...)]` keys a derived component declares:
//! its layout, whether it is saved, whether a game save keeps it, its
//! domain, its provenance, its check and its former names.
//!
//! Every key is read in one pass over the type's `#[component(...)]`
//! attributes, through the checks of `super::keys`: a key the type does not
//! take, a key given twice, or a value of the wrong kind is an error.

use quote::format_ident;
use syn::meta::ParseNestedMeta;
use syn::DeriveInput;

use super::keys::{self, Keys, Site};

/// The values `layout = "…"` takes, the default first.
const LAYOUTS: &[&str] = &["aos", "soa"];

/// The type keys that act only through the component's registration, which
/// `no_serializable` removes, with what is then lost.
const DEAD_WITHOUT_REGISTRATION: &[(&str, &str)] = &[
    ("check", "is never run"),
    (
        "provenance",
        "is never read: no scene asks whether to save it",
    ),
    ("resumable", "is never read: no game save keeps it"),
    ("formerly", "is never read: no load sees the old name"),
];

/// What the type-level `#[component(...)]` attributes say.
pub(super) struct ComponentAttrs {
    /// `layout = "soa"`: stored as a field-split column.
    pub(super) layout_soa: bool,
    /// `no_serializable`: no mirror and no registration.
    pub(super) no_serializable: bool,
    /// `resumable`: runtime state a game save keeps.
    pub(super) resumable: bool,
    /// `domain = <SemanticDomain variant>`, if declared.
    pub(super) domain: Option<syn::Ident>,
    /// `provenance = <ComponentProvenance variant>`; `Authored` when absent.
    pub(super) provenance: syn::Ident,
    /// `check = <path to a fn(&Self) -> Vec<FieldProblem>>`, if declared.
    pub(super) check: Option<syn::Path>,
    /// Each `formerly = "<old name>"`, in order.
    pub(super) formerly: Vec<syn::LitStr>,
}

impl ComponentAttrs {
    /// Reads the type-level keys of `input`.
    ///
    /// See the book: how-to/add-a-component.md, "Step 3 — Say who writes it".
    ///
    /// # Errors
    ///
    /// The first key of `input`'s `#[component(...)]` attributes that the
    /// type does not take, that is given twice (a `formerly` with the same
    /// name), whose value is missing or of another kind, or a `layout` other
    /// than `"soa"` or `"aos"`; spanned on that key or value. Under
    /// `no_serializable`, every `check`, `provenance`, `resumable` and
    /// `formerly`: they act only through the registration it removes.
    pub(super) fn of(input: &DeriveInput) -> syn::Result<Self> {
        let mut keys = Keys::new(Site::Type);
        let mut attrs = Self {
            layout_soa: false,
            no_serializable: false,
            resumable: false,
            domain: None,
            provenance: format_ident!("Authored"),
            check: None,
            formerly: Vec::new(),
        };
        for attr in &input.attrs {
            if !attr.path().is_ident("component") {
                continue;
            }
            attr.parse_nested_meta(|meta| {
                match keys.key(&meta)?.as_str() {
                    "domain" => {
                        attrs.domain = Some(keys::value(
                            &meta,
                            "domain",
                            "a `SemanticDomain` variant",
                            "domain = Physics",
                        )?);
                    }
                    "provenance" => {
                        attrs.provenance = keys::value(
                            &meta,
                            "provenance",
                            "a `ComponentProvenance` variant",
                            "provenance = Runtime",
                        )?;
                    }
                    "check" => {
                        attrs.check = Some(keys::value(
                            &meta,
                            "check",
                            "the path of a `fn(&Self) -> Vec<FieldProblem>`",
                            "check = Self::check",
                        )?);
                    }
                    "layout" => attrs.layout_soa = layout_soa(&meta)?,
                    "formerly" => {
                        let former = keys::former_name(&meta, &attrs.formerly)?;
                        attrs.formerly.push(former);
                    }
                    "resumable" => {
                        keys::flag(&meta, "resumable")?;
                        attrs.resumable = true;
                    }
                    "no_serializable" => {
                        keys::flag(&meta, "no_serializable")?;
                        attrs.no_serializable = true;
                    }
                    other => unreachable!("`{other}` is a type key with no reader"),
                }
                Ok(())
            })?;
        }
        if attrs.no_serializable {
            dead_keys(&keys)?;
        }
        Ok(attrs)
    }
}

/// Refuses each key of `keys` that `no_serializable` leaves without effect.
///
/// # Errors
///
/// One error per dead key, combined, each spanned on the key.
fn dead_keys(keys: &Keys) -> syn::Result<()> {
    let mut errors: Option<syn::Error> = None;
    for (key, lost) in DEAD_WITHOUT_REGISTRATION {
        let Some(path) = keys.written(key) else {
            continue;
        };
        let error = syn::Error::new_spanned(
            path,
            format!("`no_serializable` registers no component, so `{key}` {lost}"),
        );
        match &mut errors {
            Some(errors) => errors.combine(error),
            None => errors = Some(error),
        }
    }
    errors.map_or(Ok(()), Err)
}

/// Returns whether the `layout = "…"` key in `meta` asks for field-split columns.
///
/// # Errors
///
/// On the key when it has no value, on the value when it is not a string or
/// is neither `"soa"` nor `"aos"`.
fn layout_soa(meta: &ParseNestedMeta) -> syn::Result<bool> {
    let layout: syn::LitStr = keys::value(meta, "layout", "a string", "layout = \"soa\"")?;
    if !LAYOUTS.contains(&layout.value().as_str()) {
        return Err(syn::Error::new(
            layout.span(),
            format!(
                "unknown layout \"{}\": `layout` is \"soa\" (field-split columns) \
                 or \"aos\" (the default)",
                layout.value()
            ),
        ));
    }
    Ok(layout.value() == "soa")
}

#[cfg(test)]
mod tests {
    use syn::{parse_quote, DeriveInput};

    use super::ComponentAttrs;

    /// Returns the message of the error reading `input` gives.
    fn error_of(input: DeriveInput) -> String {
        ComponentAttrs::of(&input)
            .err()
            .expect("the keys are refused")
            .to_string()
    }

    #[test]
    fn every_type_key_is_read_across_attributes() {
        let input: DeriveInput = parse_quote! {
            #[derive(Default)]
            #[component(domain = Physics, provenance = Runtime, check = Self::check)]
            #[component(layout = "soa", formerly = "Old", formerly = "Older")]
            #[component(resumable)]
            struct Mass { kilograms: f32 }
        };
        let attrs = ComponentAttrs::of(&input).expect("every key is known");
        assert!(attrs.layout_soa);
        assert!(!attrs.no_serializable);
        assert!(attrs.resumable);
        assert_eq!(attrs.domain.expect("declared").to_string(), "Physics");
        assert_eq!(attrs.provenance.to_string(), "Runtime");
        assert!(attrs.check.is_some());
        let formerly: Vec<_> = attrs.formerly.iter().map(|name| name.value()).collect();
        assert_eq!(formerly, ["Old", "Older"]);
    }

    #[test]
    fn a_type_without_keys_takes_the_defaults() {
        let input: DeriveInput = parse_quote! {
            #[derive(Default)]
            struct Mass { kilograms: f32 }
        };
        let attrs = ComponentAttrs::of(&input).expect("no key is no error");
        assert!(!attrs.layout_soa && !attrs.no_serializable && !attrs.resumable);
        assert!(attrs.domain.is_none() && attrs.check.is_none());
        assert_eq!(attrs.provenance.to_string(), "Authored");
        assert!(attrs.formerly.is_empty());
    }

    #[test]
    fn an_unknown_type_key_names_the_type_keys() {
        let message = error_of(parse_quote! {
            #[component(domian = Physics)]
            struct Mass;
        });
        assert_eq!(
            message,
            "unknown `#[component]` key `domian` on the type: its keys are `domain`, \
             `provenance`, `check`, `layout`, `formerly`, `resumable`, `no_serializable`"
        );
    }

    #[test]
    fn a_field_key_on_the_type_says_where_it_goes() {
        let message = error_of(parse_quote! {
            #[component(skip)]
            struct Mass;
        });
        assert!(message.ends_with("; `skip` goes on a field"), "{message}");
    }

    #[test]
    fn an_unknown_key_after_known_ones_is_still_an_error() {
        let message = error_of(parse_quote! {
            #[component(domain = Physics, layout = "soa", resumabel)]
            struct Mass;
        });
        assert!(
            message.starts_with("unknown `#[component]` key `resumabel`"),
            "{message}"
        );
    }

    #[test]
    fn a_key_given_twice_across_attributes_is_an_error() {
        let message = error_of(parse_quote! {
            #[component(domain = Physics)]
            #[component(domain = Render)]
            struct Mass;
        });
        assert_eq!(message, "`domain` is given twice: keep one");
    }

    #[test]
    fn a_flag_given_twice_is_an_error() {
        let message = error_of(parse_quote! {
            #[component(resumable, resumable)]
            struct Mass;
        });
        assert_eq!(message, "`resumable` is given twice: keep one");
    }

    #[test]
    fn a_former_name_given_twice_is_an_error() {
        let message = error_of(parse_quote! {
            #[component(formerly = "Old", formerly = "Old")]
            struct Mass;
        });
        assert_eq!(message, "`formerly = \"Old\"` is given twice: keep one");
    }

    #[test]
    fn an_empty_former_name_is_an_error() {
        let message = error_of(parse_quote! {
            #[component(formerly = "")]
            struct Mass;
        });
        assert_eq!(message, "`formerly` names an old name: it cannot be empty");
    }

    #[test]
    fn a_flag_with_a_value_is_an_error() {
        let message = error_of(parse_quote! {
            #[component(no_serializable = true)]
            struct Mass;
        });
        assert_eq!(
            message,
            "`no_serializable` takes no value: `#[component(no_serializable)]`"
        );
    }

    #[test]
    fn a_value_key_without_its_value_is_an_error() {
        let message = error_of(parse_quote! {
            #[component(domain)]
            struct Mass;
        });
        assert_eq!(
            message,
            "`domain` takes a `SemanticDomain` variant: `domain = Physics`"
        );
    }

    #[test]
    fn a_value_of_another_kind_is_an_error() {
        let domain = error_of(parse_quote! {
            #[component(domain = "Physics")]
            struct Mass;
        });
        assert_eq!(
            domain,
            "`domain` takes a `SemanticDomain` variant: `domain = Physics`"
        );
        let formerly = error_of(parse_quote! {
            #[component(formerly = Old)]
            struct Mass;
        });
        assert_eq!(
            formerly,
            "`formerly` takes the old name, a string: `formerly = \"Old\"`"
        );
        let layout = error_of(parse_quote! {
            #[component(layout = soa)]
            struct Mass;
        });
        assert_eq!(layout, "`layout` takes a string: `layout = \"soa\"`");
    }

    #[test]
    fn an_unknown_layout_is_an_error() {
        let message = error_of(parse_quote! {
            #[component(layout = "SOA")]
            struct Mass;
        });
        assert_eq!(
            message,
            "unknown layout \"SOA\": `layout` is \"soa\" (field-split columns) or \"aos\" \
             (the default)"
        );
    }

    #[test]
    fn each_key_no_serializable_leaves_dead_is_an_error() {
        let check = error_of(parse_quote! {
            #[component(check = Self::check, no_serializable)]
            struct Mass;
        });
        assert_eq!(
            check,
            "`no_serializable` registers no component, so `check` is never run"
        );
        let provenance = error_of(parse_quote! {
            #[component(no_serializable, provenance = Bogus)]
            struct Mass;
        });
        assert_eq!(
            provenance,
            "`no_serializable` registers no component, so `provenance` is never read: no scene asks whether to save it"
        );
        let resumable = error_of(parse_quote! {
            #[component(resumable)]
            #[component(no_serializable)]
            struct Mass;
        });
        assert_eq!(
            resumable,
            "`no_serializable` registers no component, so `resumable` is never read: no game save keeps it"
        );
        let formerly = error_of(parse_quote! {
            #[component(no_serializable, formerly = "Old")]
            struct Mass;
        });
        assert_eq!(
            formerly,
            "`no_serializable` registers no component, so `formerly` is never read: no load sees the old name"
        );
    }

    #[test]
    fn every_dead_key_is_reported() {
        let input: DeriveInput = parse_quote! {
            #[component(no_serializable, resumable, check = Self::check, formerly = "Old")]
            #[component(provenance = Runtime)]
            struct Mass;
        };
        let errors = ComponentAttrs::of(&input)
            .err()
            .expect("the dead keys are refused");
        assert_eq!(errors.into_iter().count(), 4);
    }

    #[test]
    fn no_serializable_keeps_its_live_keys() {
        let input: DeriveInput = parse_quote! {
            #[component(no_serializable, domain = Physics, layout = "soa")]
            struct Mass { kilograms: f32 }
        };
        let attrs = ComponentAttrs::of(&input).expect("domain and layout still act");
        assert!(attrs.no_serializable && attrs.layout_soa);
    }

    #[test]
    fn the_aos_layout_is_the_default_said_aloud() {
        let input: DeriveInput = parse_quote! {
            #[component(layout = "aos")]
            struct Mass { kilograms: f32 }
        };
        assert!(
            !ComponentAttrs::of(&input)
                .expect("aos is a layout")
                .layout_soa
        );
    }
}
