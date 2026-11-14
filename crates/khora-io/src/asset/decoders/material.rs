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

//! Material decoder: `.kmat` RON bytes → `Box<dyn Material>`.
//!
//! See the book: reference/formats.md, "`.kmat` — material file".

use std::error::Error;

use khora_core::asset::Material;
use khora_data::scene::MaterialRegistration;

use crate::asset::{AssetDecoder, DecoderRegistration};

/// Decodes a `.kmat` RON document into the [`Material`] its `type_name` registers.
///
/// A missing `type_name`, an unknown type or a field that does not decode fails
/// the load: there is no fallback material.
#[derive(Clone, Default)]
pub struct MaterialDecoder;

impl AssetDecoder<Box<dyn Material>> for MaterialDecoder {
    fn sniff(&self, head: &[u8]) -> bool {
        crate::trust::looks_like_text(head)
    }

    fn limits(&self, trust: crate::trust::Trust) -> crate::trust::Limits {
        crate::trust::Limits::of(crate::trust::FileKind::Text, trust)
    }

    fn load(
        &self,
        bytes: &[u8],
    ) -> Result<Box<dyn Material>, Box<dyn Error + Send + Sync + 'static>> {
        decode_material_inner(bytes)
    }
}

/// Decodes `.kmat` RON bytes into a [`Material`] without an [`AssetService`].
///
/// For the index builder, which reads a material's texture references while it
/// scans a project. Returns `None` on any failure; [`MaterialDecoder::load`]
/// says why.
///
/// [`AssetService`]: crate::asset::AssetService
pub fn decode_material(bytes: &[u8]) -> Option<Box<dyn Material>> {
    decode_material_inner(bytes).ok()
}

fn decode_material_inner(
    bytes: &[u8],
) -> Result<Box<dyn Material>, Box<dyn Error + Send + Sync + 'static>> {
    // RON is self-describing, so it parses into a JSON value tree: the
    // registrations' `deserialize_json` reads it, with no RON path of its own.
    let doc: serde_json::Value =
        ron::de::from_bytes(bytes).map_err(|e| format!("failed to parse .kmat RON: {e}"))?;

    let type_name = doc
        .get("type_name")
        .and_then(serde_json::Value::as_str)
        .ok_or("`.kmat` missing string field `type_name`")?;

    let material_value = doc
        .get("material")
        .ok_or("`.kmat` missing field `material`")?;

    for reg in inventory::iter::<MaterialRegistration> {
        if reg.type_name == type_name {
            let material = (reg.deserialize_json)(material_value)
                .map_err(|e| format!("failed to decode `{type_name}` material: {e}"))?;
            return Ok(material);
        }
    }

    Err(format!("no MaterialRegistration found for type `{type_name}`").into())
}

inventory::submit! {
    DecoderRegistration {
        type_name: "material",
        register: |svc| {
            svc.register_decoder::<Box<dyn Material>>("material", MaterialDecoder);
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::asset::{AssetUUID, EmissiveMaterial, StandardMaterial};
    use khora_core::math::LinearRgba;
    use khora_data::scene::material_to_json;

    /// Renders a `{ type_name, material }` JSON object as a `.kmat` document.
    fn json_to_kmat_ron(value: &serde_json::Value) -> String {
        ron::ser::to_string(value).expect("material JSON should encode to RON")
    }

    #[test]
    fn decodes_hand_written_standard_kmat_with_texture() {
        // Non-default fields and a texture UUID, so a lossy bridge shows. As
        // `material_to_json` → RON writes them: an absent option is `()`, a
        // unit variant and a UUID are quoted strings.
        let tex_uuid = AssetUUID::new_v5("textures/wood.png");
        let tex_str = serde_json::to_value(tex_uuid)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .expect("AssetUUID serializes to a string");
        let kmat = format!(
            r#"{{
                "type_name": "StandardMaterial",
                "material": {{
                    "base_color": {{ "r": 0.1, "g": 0.2, "b": 0.3, "a": 1.0 }},
                    "base_color_texture": "{tex}",
                    "metallic": 0.75,
                    "roughness": 0.25,
                    "metallic_roughness_texture": (),
                    "normal_map": (),
                    "occlusion_map": (),
                    "emissive": {{ "r": 0.0, "g": 0.0, "b": 0.0, "a": 1.0 }},
                    "emissive_texture": (),
                    "alpha_mode": "Opaque",
                    "alpha_cutoff": 0.5,
                    "double_sided": false,
                }},
            }}"#,
            tex = tex_str,
        );

        let material = MaterialDecoder
            .load(kmat.as_bytes())
            .expect("hand-written StandardMaterial .kmat should decode");
        let standard = material
            .as_any()
            .downcast_ref::<StandardMaterial>()
            .expect("decoded material should be a StandardMaterial");

        assert_eq!(standard.base_color, LinearRgba::new(0.1, 0.2, 0.3, 1.0));
        assert_eq!(standard.metallic, 0.75);
        assert_eq!(standard.roughness, 0.25);
        assert_eq!(standard.base_color_texture, Some(tex_uuid));
    }

    #[test]
    fn decodes_non_standard_type_via_dispatch() {
        // A second concrete type proves the `type_name` dispatch, not a
        // hard-coded StandardMaterial path.
        let emissive = EmissiveMaterial::default();
        let json = material_to_json(&emissive).expect("emissive material should serialize to JSON");
        let kmat = json_to_kmat_ron(&json);

        let material = MaterialDecoder
            .load(kmat.as_bytes())
            .expect("EmissiveMaterial .kmat should decode");
        assert!(
            material
                .as_any()
                .downcast_ref::<EmissiveMaterial>()
                .is_some(),
            "decoded material should dispatch to EmissiveMaterial"
        );
    }

    #[test]
    fn serialize_then_decode_round_trips_all_fields() {
        // The sharp edges of the JSON value ↔ RON bridge: the
        // `AlphaMode::Mask(f32)` tuple variant and `Option<AssetUUID>`.
        let original = StandardMaterial {
            base_color: LinearRgba::new(0.42, 0.13, 0.87, 0.5),
            base_color_texture: Some(AssetUUID::new_v5("textures/diffuse.png")),
            metallic: 0.9,
            roughness: 0.11,
            normal_map: Some(AssetUUID::new_v5("textures/normal.png")),
            occlusion_map: Some(AssetUUID::new_v5("textures/ao.png")),
            alpha_mode: khora_core::asset::AlphaMode::Mask(0.33),
            alpha_cutoff: 0.33,
            double_sided: true,
            ..Default::default()
        };

        let json = material_to_json(&original).expect("material should serialize to JSON");
        let kmat = json_to_kmat_ron(&json);

        let decoded = MaterialDecoder
            .load(kmat.as_bytes())
            .expect("round-trip .kmat should decode");
        let standard = decoded
            .as_any()
            .downcast_ref::<StandardMaterial>()
            .expect("round-trip material should be a StandardMaterial");

        assert_eq!(standard.base_color, original.base_color);
        assert_eq!(standard.base_color_texture, original.base_color_texture);
        assert_eq!(standard.metallic, original.metallic);
        assert_eq!(standard.roughness, original.roughness);
        assert_eq!(standard.normal_map, original.normal_map);
        assert_eq!(standard.occlusion_map, original.occlusion_map);
        assert_eq!(standard.alpha_mode, original.alpha_mode);
        assert_eq!(standard.alpha_cutoff, original.alpha_cutoff);
        assert_eq!(standard.double_sided, original.double_sided);
    }

    /// A `.kmat` with no specular field decodes to specular 1 and white, not 0,
    /// so it renders as a material without `KHR_materials_specular` would.
    #[test]
    fn a_kmat_without_specular_fields_decodes_to_specular_one_and_white() {
        let kmat = r#"{
            "type_name": "StandardMaterial",
            "material": {
                "base_color": { "r": 0.1, "g": 0.2, "b": 0.3, "a": 1.0 },
                "base_color_texture": (),
                "metallic": 0.75,
                "roughness": 0.25,
                "metallic_roughness_texture": (),
                "normal_map": (),
                "occlusion_map": (),
                "emissive": { "r": 0.0, "g": 0.0, "b": 0.0, "a": 1.0 },
                "emissive_texture": (),
                "alpha_mode": "Opaque",
                "alpha_cutoff": 0.5,
                "double_sided": false,
            },
        }"#;

        let material = MaterialDecoder
            .load(kmat.as_bytes())
            .expect("a .kmat without specular fields still decodes");
        let standard = material
            .as_any()
            .downcast_ref::<StandardMaterial>()
            .expect("decoded material should be a StandardMaterial");

        assert_eq!(standard.metallic, 0.75, "the fields it has are kept");
        assert_eq!(standard.specular, 1.0);
        assert_eq!(standard.specular_color, LinearRgba::WHITE);
        assert_eq!(standard.specular_texture, None);
        assert_eq!(standard.specular_color_texture, None);
        assert_eq!(material.specular(), 1.0);
        assert_eq!(material.specular_color(), LinearRgba::WHITE);
    }

    #[test]
    fn unknown_type_name_errors_without_fallback() {
        let kmat = r#"(type_name: "NoSuchMaterial", material: ())"#;
        assert!(
            MaterialDecoder.load(kmat.as_bytes()).is_err(),
            "unknown material type must error, never fall back to a default"
        );
    }

    #[test]
    fn malformed_ron_errors() {
        assert!(
            MaterialDecoder.load(b"this is not ron").is_err(),
            "malformed RON must surface a decode error"
        );
    }
}
