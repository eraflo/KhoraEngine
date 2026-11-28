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

//! The registrations of the asset references that are components: an
//! entity's mesh and its material.

use khora_core::asset::{InlineMaterial, Material, ProceduralMesh, StandardMaterial};
use khora_core::renderer::api::gpu_scene::Mesh;

/// Registers `AssetRef<$asset>` as a component of `$domain`, saved under its
/// own name (`AssetRef<Mesh>`), its default `$default`.
///
/// A value made in code is session-only: a scene leaves it out, a load drops it.
macro_rules! register_asset_ref {
    ($asset:ty, domain = $domain:ident, default = $default:expr $(,)?) => {
        const _: () = {
            /// The name the reference is saved and registered under.
            const NAME: &str = concat!("AssetRef<", stringify!($asset), ">");

            /// The schema of the reference's positional form, traced once.
            static SCHEMA: std::sync::OnceLock<(u64, bool)> = std::sync::OnceLock::new();

            /// Returns the schema of the reference's positional form.
            fn schema() -> (u64, bool) {
                *SCHEMA.get_or_init(super::saved::schema::<$asset>)
            }

            inventory::submit! {
                crate::ecs::ComponentDomainRegistration {
                    register: |world| {
                        world.register_component::<super::AssetRef<$asset>>(
                            crate::ecs::SemanticDomain::$domain,
                        );
                    },
                }
            }

            inventory::submit! {
                crate::scene::ComponentRegistration {
                    type_id: std::any::TypeId::of::<super::AssetRef<$asset>>(),
                    type_name: NAME,
                    // One of several forms, not a struct of fields. See `ComponentShape`.
                    shape: crate::scene::ComponentShape::Opaque,
                    provenance: crate::ecs::ComponentProvenance::Authored,
                    formerly: &[],
                    resumable: false,
                    column_to_record: |column, row, references| {
                        super::saved::to_record::<$asset>(NAME, column, row, references)
                    },
                    stage: super::saved::stage::<$asset>,
                    schema: || schema().0,
                    schema_complete: || schema().1,
                    column_to_snapshot: super::saved::to_snapshot::<$asset>,
                    stage_snapshot: super::saved::stage_snapshot::<$asset>,
                    create_default: |world, entity| {
                        world
                            .add_component(entity, $default)
                            .map(|_| ())
                            .map_err(|error| format!("{error:?}"))
                    },
                    to_json: super::saved::to_json::<$asset>,
                    from_json: super::saved::from_json::<$asset>,
                    remove: super::saved::remove::<$asset>,
                    // One of several forms: no field a script reaches.
                    script_type: |_| None,
                    script_default: || None,
                    check: None,
                    session_only: Some(super::saved::is_made::<$asset>),
                }
            }
        };
    };
}

register_asset_ref!(
    Mesh,
    domain = Render,
    default = super::AssetRef::<Mesh>::Local(ProceduralMesh::unit_cube()),
);

register_asset_ref!(
    Box<dyn Material>,
    domain = Render,
    default = super::AssetRef::<Box<dyn Material>>::Local(InlineMaterial::new(Box::new(
        StandardMaterial::default()
    ))),
);

#[cfg(test)]
mod tests {
    use khora_core::asset::Material;
    use khora_core::renderer::api::gpu_scene::Mesh;

    use crate::ecs::component_registry::short_name;
    use crate::ecs::AssetRef;
    use crate::scene::ComponentRegistration;

    /// Each reference's registration is named as the world names its type.
    #[test]
    fn a_reference_is_registered_under_its_short_name() {
        for (type_id, path) in [
            (
                std::any::TypeId::of::<AssetRef<Mesh>>(),
                std::any::type_name::<AssetRef<Mesh>>(),
            ),
            (
                std::any::TypeId::of::<AssetRef<Box<dyn Material>>>(),
                std::any::type_name::<AssetRef<Box<dyn Material>>>(),
            ),
        ] {
            let registration = inventory::iter::<ComponentRegistration>
                .into_iter()
                .find(|registration| registration.type_id == type_id)
                .unwrap_or_else(|| panic!("`{path}` is not registered"));
            assert_eq!(registration.type_name, short_name(path));
        }
    }
}
