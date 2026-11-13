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

use super::*;
use crate::math::EPSILON;

fn approx_eq(a: f32, b: f32) -> bool {
    (a - b).abs() < EPSILON
}

#[test]
fn test_directional_light_default() {
    let light = DirectionalLight::default();
    assert_eq!(light.color, LinearRgba::WHITE);
    assert!(approx_eq(light.intensity, 1.0));
    // Direction should be normalized
    assert!(approx_eq(light.direction.length(), 1.0));
}

#[test]
fn test_directional_light_custom() {
    let direction = Vec3::new(1.0, -1.0, 0.0).normalize();
    let light = DirectionalLight {
        direction,
        color: LinearRgba::new(1.0, 0.5, 0.0, 1.0),
        intensity: 2.0,
        shadow_enabled: false,
        shadow_bias: 0.005,
        shadow_normal_bias: 0.0,
    };
    assert!(approx_eq(light.direction.length(), 1.0));
    assert!(approx_eq(light.intensity, 2.0));
}

#[test]
fn test_point_light_default() {
    let light = PointLight::default();
    assert_eq!(light.color, LinearRgba::WHITE);
    assert!(approx_eq(light.intensity, 100.0));
    assert!(approx_eq(light.range, 10.0));
}

#[test]
fn test_point_light_custom() {
    let light = PointLight {
        color: LinearRgba::new(0.0, 1.0, 0.0, 1.0),
        intensity: 50.0,
        range: 5.0,
        shadow_enabled: false,
        shadow_bias: 0.01,
        shadow_normal_bias: 0.0,
    };
    assert!(approx_eq(light.intensity, 50.0));
    assert!(approx_eq(light.range, 5.0));
}

#[test]
fn test_spot_light_default() {
    let light = SpotLight::default();
    assert_eq!(light.color, LinearRgba::WHITE);
    assert!(light.inner_cone_angle < light.outer_cone_angle);
    assert!(approx_eq(light.direction.length(), 1.0));
}

#[test]
fn test_spot_light_cone_angles() {
    let light = SpotLight {
        inner_cone_angle: 10.0_f32.to_radians(),
        outer_cone_angle: 45.0_f32.to_radians(),
        ..Default::default()
    };
    assert!(light.inner_cone_angle < light.outer_cone_angle);
    assert!(light.outer_cone_angle < std::f32::consts::FRAC_PI_2);
}

#[test]
fn test_light_type_default() {
    let light = LightType::default();
    match light {
        LightType::Directional(_) => {}
        _ => panic!("Expected Directional light as default"),
    }
}

#[test]
fn test_light_type_variants() {
    let dir = LightType::Directional(DirectionalLight::default());
    let point = LightType::Point(PointLight::default());
    let spot = LightType::Spot(SpotLight::default());

    assert!(matches!(dir, LightType::Directional(_)));
    assert!(matches!(point, LightType::Point(_)));
    assert!(matches!(spot, LightType::Spot(_)));
}
