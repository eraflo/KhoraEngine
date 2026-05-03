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

//! Registry of inspector-editable enum variants.
//!
//! The JSON-driven inspector walks `serde_json::Value`s and renders widgets
//! by shape. Single-key objects are typically serde-tagged enum variants:
//! e.g. `{"Directional": {...}}` for `LightType::Directional(_)`. To let
//! the user *switch* the variant from the inspector, we need (1) the list
//! of valid variant names and (2) a default-serialized payload for each.
//!
//! That information cannot be reliably derived from the JSON alone (the
//! type information is lost once we serialize to `serde_json::Value`), so
//! this module hard-codes the small set of enums the inspector currently
//! knows how to switch. It is **opt-in**: enums not registered here keep
//! the previous read-only `Variant: <key>` label.
//!
//! A future iteration could move this generation into the `#[derive(Component)]`
//! macro itself (emit `enum_variants_for(field_path)` per field whose type
//! is an enum). For now, the registry is the smallest honest fix.

use khora_sdk::khora_core::renderer::light::{
    DirectionalLight, LightType, PointLight, SpotLight,
};
use serde_json::Value;
use std::sync::OnceLock;

/// Returns the editable variant set for a single-key JSON object whose key
/// matches a known enum variant, or `None` if the key isn't recognised.
///
/// Each entry is `(variant_name, full_default_json)`. The
/// `full_default_json` is the entire `{"VariantName": {...}}` object —
/// callers replace the current map with it on selection.
pub fn editable_variants(current_key: &str) -> Option<&'static [(&'static str, Value)]> {
    static REGISTRY: OnceLock<Vec<RegisteredEnum>> = OnceLock::new();
    let registry = REGISTRY.get_or_init(build_registry);
    registry
        .iter()
        .find(|e| e.variants.iter().any(|(name, _)| *name == current_key))
        .map(|e| e.variants.as_slice())
}

struct RegisteredEnum {
    variants: Vec<(&'static str, Value)>,
}

fn build_registry() -> Vec<RegisteredEnum> {
    vec![
        // LightType — Directional / Point / Spot (Light::light_type).
        RegisteredEnum {
            variants: vec![
                (
                    "Directional",
                    serde_json::to_value(LightType::Directional(DirectionalLight::default()))
                        .expect("Directional default serialises"),
                ),
                (
                    "Point",
                    serde_json::to_value(LightType::Point(PointLight::default()))
                        .expect("Point default serialises"),
                ),
                (
                    "Spot",
                    serde_json::to_value(LightType::Spot(SpotLight::default()))
                        .expect("Spot default serialises"),
                ),
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_light_keys_are_recognised() {
        for key in ["Directional", "Point", "Spot"] {
            let variants = editable_variants(key).unwrap_or_else(|| panic!("missing {}", key));
            assert_eq!(variants.len(), 3);
        }
    }

    #[test]
    fn unknown_keys_return_none() {
        assert!(editable_variants("Bogus").is_none());
        assert!(editable_variants("").is_none());
    }

    #[test]
    fn variant_payloads_are_single_key_objects() {
        for (_, default) in editable_variants("Directional").unwrap() {
            let Value::Object(map) = default else {
                panic!("default not an object");
            };
            assert_eq!(map.len(), 1);
        }
    }
}
