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

//! The `Component` methods through which a script reaches a component's
//! fields, generated for `#[derive(Component)]`.

use quote::{quote, ToTokens};
use syn::Fields;

/// Returns the `Component` methods a script reaches the included fields through, by slot.
///
/// The same list builds the `Serializable` mirror and the field schema, so a
/// skipped field is out of all three. A field whose type is not a
/// `ScriptField` resolves to "not accessible" rather than a compile error.
pub(crate) fn script_methods(fields: &Fields, included_fields: &[&syn::Field]) -> impl ToTokens {
    let positions: Vec<usize> = fields
        .iter()
        .enumerate()
        .filter(|(_, field)| {
            included_fields
                .iter()
                .any(|kept| std::ptr::eq(*kept, *field))
        })
        .map(|(position, _)| position)
        .collect();
    let names: Vec<String> = match fields {
        Fields::Named(_) => included_fields
            .iter()
            .map(|f| f.ident.as_ref().expect("named field").to_string())
            .collect(),
        _ => (0..included_fields.len()).map(|i| i.to_string()).collect(),
    };
    let access: Vec<_> = included_fields
        .iter()
        .zip(&positions)
        .map(|(f, position)| match &f.ident {
            Some(ident) => quote! { #ident },
            None => {
                let index = syn::Index::from(*position);
                quote! { #index }
            }
        })
        .collect();
    let types: Vec<&syn::Type> = included_fields.iter().map(|f| &f.ty).collect();
    let slots: Vec<usize> = (0..included_fields.len()).collect();
    quote! {
        fn script_fields() -> &'static [&'static str] {
            &[#(#names),*]
        }
        fn script_type(slot: usize) -> ::core::option::Option<::khora_core::script::ErgonType> {
            #[allow(unused_imports)]
            use ::khora_core::script::field::probe::{Probe, ViaNothing as _, ViaScriptField as _};
            match slot {
                #(#slots => (&&Probe::<#types>::new()).ergon_of(),)*
                _ => ::core::option::Option::None,
            }
        }
        fn read_field(&self, slot: usize) -> ::core::option::Option<::khora_core::script::ScriptValue> {
            #[allow(unused_imports)]
            use ::khora_core::script::field::probe::{Probe, ViaNothing as _, ViaScriptField as _};
            match slot {
                #(#slots => (&&Probe::<#types>::new()).reader().map(|read| read(&self.#access)),)*
                _ => ::core::option::Option::None,
            }
        }
        fn check_field(
            slot: usize,
            value: &::khora_core::script::ScriptValue,
        ) -> ::core::result::Result<(), ::khora_core::script::FieldValueError> {
            #[allow(unused_imports)]
            use ::khora_core::script::field::probe::{Probe, ViaNothing as _, ViaScriptField as _};
            let refused = || ::khora_core::script::FieldValueError {
                expected: "a field a script can write".to_owned(),
                found: value.type_name().to_owned(),
            };
            match slot {
                #(#slots => match (&&Probe::<#types>::new()).writer() {
                    ::core::option::Option::Some(write) => write(value).map(|_| ()),
                    ::core::option::Option::None => ::core::result::Result::Err(refused()),
                },)*
                _ => ::core::result::Result::Err(refused()),
            }
        }
        fn write_field(
            &mut self,
            slot: usize,
            value: &::khora_core::script::ScriptValue,
        ) -> ::core::result::Result<(), ::khora_core::script::FieldValueError> {
            #[allow(unused_imports)]
            use ::khora_core::script::field::probe::{Probe, ViaNothing as _, ViaScriptField as _};
            let refused = || ::khora_core::script::FieldValueError {
                expected: "a field a script can write".to_owned(),
                found: value.type_name().to_owned(),
            };
            match slot {
                #(#slots => match (&&Probe::<#types>::new()).writer() {
                    ::core::option::Option::Some(write) => {
                        self.#access = write(value)?;
                        ::core::result::Result::Ok(())
                    }
                    ::core::option::Option::None => ::core::result::Result::Err(refused()),
                },)*
                _ => ::core::result::Result::Err(refused()),
            }
        }
    }
}

/// Returns the registration's `script_default`: the component's `Default`, as a script reads it.
///
/// A script's `Add` starts from it in the frame's working set.
pub(crate) fn script_default(name: &syn::Ident) -> impl ToTokens {
    quote! { || {
        let value = <#name as ::core::default::Default>::default();
        let fields = <#name as crate::ecs::component::Component>::script_fields();
        ::core::option::Option::Some(::khora_core::script::ScriptValue::Struct(
            fields
                .iter()
                .enumerate()
                .filter_map(|(slot, field)| {
                    ::core::option::Option::Some((
                        (*field).to_owned(),
                        crate::ecs::component::Component::read_field(&value, slot)?,
                    ))
                })
                .collect(),
        ))
    } }
}
