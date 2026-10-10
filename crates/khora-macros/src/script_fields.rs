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

/// What a script reaches: the included fields, by slot — the very list the
/// `Serializable` mirror and the field schema are built from, so a skipped
/// field is out of every one of them. A field whose type is not a
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
