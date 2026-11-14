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

//! The enums whose variant the inspector can switch, each variant with its
//! default payload. Listed by hand, since a JSON value has lost its type; an
//! enum not listed here is shown read-only.

use khora_sdk::khora_core::renderer::light::{DirectionalLight, LightType, PointLight, SpotLight};
use serde_json::Value;
use std::sync::OnceLock;

/// Returns the variants of the enum whose variant is named `current_key`, or
/// `None` for an unknown name.
///
/// Each entry is `(name, default)`, `default` being the whole
/// `{"Name": {...}}` object that replaces the current one when picked.
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
