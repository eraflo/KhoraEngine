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

//! The `inventory` registrations a derived component submits: its domain,
//! and the `ComponentRegistration` through which it is saved, loaded,
//! snapshotted and inspected by name.

use quote::{quote, ToTokens};

use super::attrs::ComponentAttrs;

/// The domain registration of `name`, when it declares
/// `#[component(domain = …)]`; nothing otherwise.
pub(super) fn domain_registration(name: &syn::Ident, domain: Option<&syn::Ident>) -> impl ToTokens {
    match domain {
        Some(domain) => quote! {
            inventory::submit! {
                crate::ecs::ComponentDomainRegistration {
                    register: |world: &mut crate::ecs::World| {
                        world.register_component::<#name>(crate::ecs::SemanticDomain::#domain);
                    }
                }
            }
        },
        None => quote! {},
    }
}

/// The `ComponentRegistration` of `name`, saved through its mirror
/// `serializable_name`, with its `field_schema`, its schema traced once into
/// the static `schema_cell`, and its declared `check`.
pub(super) fn component_registration(
    name: &syn::Ident,
    serializable_name: &syn::Ident,
    attrs: &ComponentAttrs,
    field_schema: &impl ToTokens,
    schema_cell: &syn::Ident,
    check_path: Option<&syn::Path>,
) -> impl ToTokens {
    let provenance_ident = &attrs.provenance;
    let formerly = &attrs.formerly;
    let resumable = attrs.resumable;
    let script_default = crate::script_fields::script_default(name);

    let (stage_check, registration_check) = crate::component_check::check_tokens(name, check_path);
    quote! {
        // Auto-register this component for saves AND for the editor's
        // generic inspector — both through serde on the same Serializable
        // mirror, so anything tagged `#[component(skip)]` is omitted from both.
        inventory::submit! {
            crate::scene::ComponentRegistration {
                type_id: std::any::TypeId::of::<#name>(),
                type_name: stringify!(#name),
                shape: #field_schema,
                provenance: crate::ecs::ComponentProvenance::#provenance_ident,
                formerly: &[#(#formerly),*],
                resumable: #resumable,
                column_to_record: |column, row, references| {
                    // By value so it works for any column layout (AoS or field-SoA).
                    let value = <#name as crate::ecs::component::Component>::clone_from_column(
                        column, row,
                    );
                    crate::scene::record::to_record(&<#serializable_name>::from(value), references)
                },
                stage: |record, references| {
                    let (s, mut report) = crate::scene::record::resolve::<#serializable_name>(
                        record, references,
                    )?;
                    let value = <#name>::from(s);
                    #stage_check
                    Ok(crate::scene::StagedComponent {
                        component: Box::new(crate::scene::StagedValue(value)),
                        report,
                    })
                },
                schema: || {
                    crate::scene::schema::traced_once::<#serializable_name>(
                        &#schema_cell,
                        stringify!(#name),
                    )
                    .0
                },
                schema_complete: || {
                    crate::scene::schema::traced_once::<#serializable_name>(
                        &#schema_cell,
                        stringify!(#name),
                    )
                    .1
                },
                column_to_snapshot: |column, row, out, references| {
                    let value = <#name as crate::ecs::component::Component>::clone_from_column(
                        column, row,
                    );
                    crate::scene::positional::to_positional(
                        &<#serializable_name>::from(value),
                        out,
                        references,
                    )
                },
                stage_snapshot: |bytes, references| {
                    let s = crate::scene::positional::from_positional::<#serializable_name>(
                        bytes, references,
                    )?;
                    Ok(crate::scene::StagedComponent {
                        component: Box::new(crate::scene::StagedValue(<#name>::from(s))),
                        report: Vec::new(),
                    })
                },
                create_default: |world, entity| {
                    // Reported, not swallowed: a caller told "attached" must
                    // find the component there.
                    world
                        .add_component(entity, <#name>::default())
                        .map(|_| ())
                        .map_err(|error| format!("{error:?}"))
                },
                to_json: |world, entity| {
                    world.clone_component::<#name>(entity).and_then(|c| {
                        crate::scene::inspector_json::to_json(&<#serializable_name>::from(c))
                    })
                },
                from_json: |world, entity, value| {
                    let s: #serializable_name = crate::scene::inspector_json::from_json(value)?;
                    let new_value = <#name>::from(s);
                    // Layout-agnostic write: update in place if present, else add.
                    if !world.set_component(entity, new_value.clone()) {
                        world.add_component(entity, new_value)
                            .map_err(|e| format!("{:?}", e))?;
                    }
                    Ok(())
                },
                remove: |world, entity| {
                    // Surgical single-component remove — preserves every
                    // other component on the entity (any domain).
                    match world.remove_component::<#name>(entity) {
                        Ok(_) => Ok(()),
                        Err(e) => Err(format!("{:?}", e)),
                    }
                },
                script_type: <#name as crate::ecs::component::Component>::script_type,
                script_default: #script_default,
                check: #registration_check,
            }
        }
    }
}
