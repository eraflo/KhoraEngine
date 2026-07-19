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
//! A `.kmat` file is RON of the shape
//! `(type_name: "StandardMaterial", material: ( ...fields... ))` — the same
//! `{ type_name, material }` split [`material_to_json`] produces, but
//! RON-encoded. The `type_name` selects the matching `MaterialRegistration`
//! from the open type-tag registry (re-exported from `khora-data`); that
//! registration's `deserialize_json` decodes the `material` sub-value into the
//! concrete material type. Any material type that registers itself (including
//! plugin types) is therefore loadable from a `.kmat` with no decoder change.
//!
//! Decoding goes through `serde_json::Value`: RON is self-describing, so
//! `ron::de::from_bytes::<serde_json::Value>` yields a serde value tree from
//! which `type_name` (a string) and `material` (a sub-tree) are pulled, and the
//! sub-tree is handed straight to `deserialize_json`. This reuses the existing
//! `serialize_json`/`deserialize_json` machinery the editor inspector already
//! relies on — no second, RON-specific code path on `MaterialRegistration`.
//!
//! Auto-registered via [`inventory::submit!`] under the canonical `"material"`
//! slot (the extension mapping `kmat|mat → "material"` lives in the index
//! builder). No fallback material: a missing `type_name`, an unknown type, or a
//! decode failure is returned as an error and the caller skips the asset.

use std::error::Error;

use khora_core::asset::Material;
use khora_data::ecs::MaterialRegistration;

use crate::asset::{AssetDecoder, DecoderRegistration};

/// Decodes a `.kmat` RON document into a type-erased [`Material`] via the
/// `MaterialRegistration` type-tag dispatch.
#[derive(Clone, Default)]
pub struct MaterialDecoder;

impl AssetDecoder<Box<dyn Material>> for MaterialDecoder {
    fn load(
        &self,
        bytes: &[u8],
    ) -> Result<Box<dyn Material>, Box<dyn Error + Send + Sync + 'static>> {
        decode_material_inner(bytes)
    }
}

/// Decodes `.kmat` RON bytes into a [`Material`] without the runtime
/// [`AssetService`], returning `None` on any failure.
///
/// The index builder calls this to read a material's texture references while
/// scanning a project, where spinning up the asset service would be both
/// heavyweight and circular. Because [`MaterialDecoder`] is stateless and
/// dispatches purely through the `inventory` type-tag registry, decoding needs
/// nothing but the bytes. A decode failure is intentionally swallowed (the
/// caller logs and continues); use [`MaterialDecoder::load`] when the error
/// detail matters.
///
/// [`AssetService`]: crate::asset::AssetService
pub fn decode_material(bytes: &[u8]) -> Option<Box<dyn Material>> {
    decode_material_inner(bytes).ok()
}

/// Shared `.kmat` RON → [`Material`] decode used by both the [`AssetDecoder`]
/// impl and the service-free [`decode_material`] helper.
fn decode_material_inner(
    bytes: &[u8],
) -> Result<Box<dyn Material>, Box<dyn Error + Send + Sync + 'static>> {
    // RON is self-describing, so it deserializes straight into a serde
    // value tree. From there we read the `{ type_name, material }` split.
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
    use khora_data::ecs::material_to_json;

    /// Renders a `{ type_name, material }` JSON object as a `.kmat` RON
    /// document — the on-disk form the decoder consumes.
    fn json_to_kmat_ron(value: &serde_json::Value) -> String {
        ron::ser::to_string(value).expect("material JSON should encode to RON")
    }

    #[test]
    fn decodes_hand_written_standard_kmat_with_texture() {
        // A hand-authored `.kmat` with distinctive, non-default fields,
        // including a `base_color_texture` UUID, to prove every field — and
        // the `Option<AssetUUID>` — survives the RON → value → material bridge.
        //
        // The `.kmat` is RON of the `{ type_name, material }` JSON-value tree,
        // so absent options serialize as `()` (the unit value), unit enum
        // variants as quoted strings, and UUIDs as quoted strings — exactly
        // what `material_to_json` → RON produces.
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
            material.as_any().downcast_ref::<EmissiveMaterial>().is_some(),
            "decoded material should dispatch to EmissiveMaterial"
        );
    }

    #[test]
    fn serialize_then_decode_round_trips_all_fields() {
        // Produce the `.kmat` bytes programmatically from a material, feed them
        // back through the decoder, and assert equality. This guards the
        // serde_json::Value ↔ RON bridge against any lossy field (the
        // `AlphaMode::Mask(f32)` tuple variant and `Option<AssetUUID>` are the
        // sharp edges).
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
