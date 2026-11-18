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

//! A derived component's fields, split into those the mirror saves and those
//! `#[component(skip)]` leaves out, with each saved field's former names.

use syn::Fields;

use super::keys::{self, Keys, Site};

/// The fields of a component, as the mirror sees them.
pub(super) struct FieldSplit<'a> {
    /// The fields the mirror holds, in declaration order.
    pub(super) included: Vec<&'a syn::Field>,
    /// The fields tagged `#[component(skip)]`, in declaration order.
    pub(super) skipped: Vec<&'a syn::Field>,
    /// Each included field's former names, which the mirror reads as serde aliases.
    ///
    /// A save written before the rename still fills the field. Only a named
    /// field has former names: a tuple field has no name to rename.
    pub(super) formerly: Vec<Vec<syn::LitStr>>,
}

impl<'a> FieldSplit<'a> {
    /// Splits `fields` by their `#[component(...)]` field attributes; `no_serializable` says the
    /// component has no registration.
    ///
    /// # Errors
    ///
    /// Every field's first wrong key, combined: a key a field does not take,
    /// a `skip` given twice, a `formerly` repeating a name or without a
    /// string, a `formerly` on a tuple field (it has no name to rename) or on
    /// a skipped field (it is not saved) or under `no_serializable` (no load
    /// reads it).
    pub(super) fn of(fields: &'a Fields, no_serializable: bool) -> syn::Result<Self> {
        let mut split = Self {
            included: Vec::new(),
            skipped: Vec::new(),
            formerly: Vec::new(),
        };
        let mut errors: Option<syn::Error> = None;
        for field in fields.iter() {
            match field_keys(field, no_serializable) {
                Ok((true, _)) => split.skipped.push(field),
                Ok((false, former)) => {
                    split.included.push(field);
                    split.formerly.push(former);
                }
                Err(error) => match &mut errors {
                    Some(errors) => errors.combine(error),
                    None => errors = Some(error),
                },
            }
        }
        match errors {
            Some(errors) => Err(errors),
            None => Ok(split),
        }
    }
}

/// Returns whether `field` is skipped, and its former names.
fn field_keys(field: &syn::Field, no_serializable: bool) -> syn::Result<(bool, Vec<syn::LitStr>)> {
    let mut keys = Keys::new(Site::Field);
    let mut skip = false;
    let mut former = Vec::new();
    for attr in &field.attrs {
        if !attr.path().is_ident("component") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            match keys.key(&meta)?.as_str() {
                "skip" => {
                    keys::flag(&meta, "skip")?;
                    skip = true;
                }
                "formerly" => {
                    if no_serializable {
                        return Err(meta.error(
                            "`no_serializable` registers no component, so a field's `formerly` is never read: no load sees the old name",
                        ));
                    }
                    let name = keys::former_name(&meta, &former)?;
                    if field.ident.is_none() {
                        return Err(syn::Error::new(
                            name.span(),
                            "a tuple field has no name: `formerly` has nothing to rename",
                        ));
                    }
                    former.push(name);
                }
                other => unreachable!("`{other}` is a field key with no reader"),
            }
            Ok(())
        })?;
    }
    if let (true, Some(name)) = (skip, former.first()) {
        return Err(syn::Error::new(
            name.span(),
            "a skipped field is not saved: `formerly` has nothing to rename",
        ));
    }
    Ok((skip, former))
}

#[cfg(test)]
mod tests {
    use syn::{parse_quote, DeriveInput};

    use super::FieldSplit;

    /// Returns the fields of `input`, a struct.
    fn fields_of(input: &DeriveInput) -> &syn::Fields {
        match &input.data {
            syn::Data::Struct(data) => &data.fields,
            _ => unreachable!("the tests derive structs"),
        }
    }

    /// Returns the message of the error splitting `input`'s fields gives.
    fn error_of(input: DeriveInput) -> String {
        FieldSplit::of(fields_of(&input), false)
            .err()
            .expect("the keys are refused")
            .to_string()
    }

    #[test]
    fn skip_and_formerly_split_the_fields() {
        let input: DeriveInput = parse_quote! {
            struct Light {
                #[component(formerly = "power", formerly = "strength")]
                intensity: f32,
                #[component(skip)]
                handle: u64,
                color: u32,
            }
        };
        let split = FieldSplit::of(fields_of(&input), false).expect("every key is known");
        let included: Vec<_> = split
            .included
            .iter()
            .map(|field| field.ident.as_ref().expect("named").to_string())
            .collect();
        assert_eq!(included, ["intensity", "color"]);
        assert_eq!(split.skipped.len(), 1);
        let formerly: Vec<Vec<String>> = split
            .formerly
            .iter()
            .map(|names| names.iter().map(|name| name.value()).collect())
            .collect();
        assert_eq!(formerly, [vec!["power", "strength"], vec![]]);
    }

    #[test]
    fn an_unknown_field_key_names_the_field_keys() {
        let message = error_of(parse_quote! {
            struct Light { #[component(skp)] handle: u64 }
        });
        assert_eq!(
            message,
            "unknown `#[component]` key `skp` on a field: its keys are `skip`, `formerly`"
        );
    }

    #[test]
    fn a_type_key_on_a_field_says_where_it_goes() {
        let message = error_of(parse_quote! {
            struct Light { #[component(domain = Render)] handle: u64 }
        });
        assert!(
            message.ends_with("; `domain` goes on the type"),
            "{message}"
        );
    }

    #[test]
    fn skip_given_twice_is_an_error() {
        let message = error_of(parse_quote! {
            struct Light {
                #[component(skip)]
                #[component(skip)]
                handle: u64,
            }
        });
        assert_eq!(message, "`skip` is given twice: keep one");
    }

    #[test]
    fn skip_with_a_value_is_an_error() {
        let message = error_of(parse_quote! {
            struct Light { #[component(skip = true)] handle: u64 }
        });
        assert_eq!(message, "`skip` takes no value: `#[component(skip)]`");
    }

    #[test]
    fn a_former_name_on_a_tuple_field_is_an_error() {
        let message = error_of(parse_quote! {
            struct Meters(#[component(formerly = "feet")] f32);
        });
        assert_eq!(
            message,
            "a tuple field has no name: `formerly` has nothing to rename"
        );
    }

    #[test]
    fn a_former_name_on_a_skipped_field_is_an_error() {
        let message = error_of(parse_quote! {
            struct Light {
                #[component(formerly = "gpu", skip)]
                handle: u64,
            }
        });
        assert_eq!(
            message,
            "a skipped field is not saved: `formerly` has nothing to rename"
        );
    }

    #[test]
    fn a_former_field_name_without_a_registration_is_an_error() {
        let input: DeriveInput = parse_quote! {
            struct Light {
                #[component(formerly = "power")]
                intensity: f32,
            }
        };
        let message = FieldSplit::of(fields_of(&input), true)
            .err()
            .expect("the dead key is refused")
            .to_string();
        assert_eq!(
            message,
            "`no_serializable` registers no component, so a field's `formerly` is never read: no load sees the old name"
        );
    }

    #[test]
    fn skip_without_a_registration_is_allowed() {
        let input: DeriveInput = parse_quote! {
            struct Light { #[component(skip)] handle: u64, intensity: f32 }
        };
        let split = FieldSplit::of(fields_of(&input), true).expect("skip still splits");
        assert_eq!(split.skipped.len(), 1);
    }

    #[test]
    fn every_wrong_field_is_reported() {
        let input: DeriveInput = parse_quote! {
            struct Light {
                #[component(skp)]
                handle: u64,
                #[component(fromerly = "power")]
                intensity: f32,
            }
        };
        let errors = FieldSplit::of(fields_of(&input), false)
            .err()
            .expect("the keys are refused");
        assert_eq!(errors.into_iter().count(), 2);
    }
}
