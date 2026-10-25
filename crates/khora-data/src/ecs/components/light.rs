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

//! Defines the Light component for the ECS.
//!
//! This component represents a light source attached to an entity in the scene.
//! The entity's `GlobalTransform` provides the position and orientation of the light.

use khora_core::math::{Mat4, Vec3, FRAC_PI_2};
use khora_core::problems::ProblemSeverity;
use khora_core::renderer::light::LightType;
use khora_macros::Component;

use crate::ecs::FieldProblem;

/// A component that adds a light source to an entity.
///
/// This component works in conjunction with the entity's `GlobalTransform`
/// to determine the light's world-space position and orientation.
///
/// # Examples
///
/// ```ignore
/// use khora_data::ecs::components::Light;
/// use khora_core::renderer::light::{LightType, DirectionalLight};
///
/// // Create a sun light
/// let sun_light = Light {
///     light_type: LightType::Directional(DirectionalLight::default()),
///     enabled: true,
/// };
/// ```
#[derive(Debug, Clone, Component)]
#[component(domain = Render, check = Light::check)]
pub struct Light {
    /// The type and properties of the light source.
    pub light_type: LightType,

    /// Whether the light is currently active.
    ///
    /// Disabled lights are not extracted for rendering and have no
    /// performance impact on the scene.
    pub enabled: bool,
}

impl Default for Light {
    fn default() -> Self {
        Self {
            light_type: LightType::default(),
            enabled: true,
        }
    }
}

impl Light {
    /// Creates a new enabled light with the given type.
    pub fn new(light_type: LightType) -> Self {
        Self {
            light_type,
            enabled: true,
        }
    }

    /// Creates a new directional light (sun-like).
    pub fn directional() -> Self {
        Self::new(LightType::Directional(
            khora_core::renderer::light::DirectionalLight::default(),
        ))
    }

    /// Creates a new point light.
    pub fn point() -> Self {
        Self::new(LightType::Point(
            khora_core::renderer::light::PointLight::default(),
        ))
    }

    /// Creates a new spot light.
    pub fn spot() -> Self {
        Self::new(LightType::Spot(
            khora_core::renderer::light::SpotLight::default(),
        ))
    }

    /// What this light's values cannot be used for, field by field — never
    /// rewritten, never refused.
    ///
    /// Each problem names the field by its path in the light's serialized
    /// form (`light_type.<Variant>.<field>`) and the value in its message:
    ///
    /// - a point or spot `range` that is not finite or is `<= 0` is an
    ///   `Error` (`light_type.Point.range` / `light_type.Spot.range`);
    /// - a spot `outer_cone_angle` that is not finite or lies outside
    ///   `(0, π/2)` is an `Error`; an `inner_cone_angle` outside
    ///   `[0, outer_cone_angle]` is an `Error`;
    /// - a shadow-casting point or spot light whose range is positive but
    ///   does not exceed [`Mat4::SHADOW_NEAR`](khora_core::math::Mat4::SHADOW_NEAR)
    ///   is a `Warning`: it lights, but casts no shadow ("casts no shadow:
    ///   range must exceed 0.1");
    /// - a directional or spot `direction` that is not finite or is zero is
    ///   an `Error` (`light_type.<Variant>.direction`);
    /// - an `intensity` that is not finite or is negative, or a colour whose
    ///   red, green or blue is, is an `Error` (`light_type.<Variant>.intensity`
    ///   / `.color`) — and so is an intensity whose product with the colour
    ///   overflows (on `.intensity`);
    /// - a `shadow_bias` or `shadow_normal_bias` that is not finite is an
    ///   `Error` (`light_type.<Variant>.shadow_bias` / `.shadow_normal_bias`).
    ///
    /// A light with an `Error` does not shine
    /// ([`shining_lights`](crate::render::shining_lights)).
    pub fn check(&self) -> Vec<FieldProblem> {
        let mut problems = Vec::new();
        match &self.light_type {
            LightType::Directional(sun) => {
                check_direction(
                    &mut problems,
                    "light_type.Directional.direction",
                    sun.direction,
                );
            }
            LightType::Point(point) => {
                check_range(
                    &mut problems,
                    "light_type.Point.range",
                    point.range,
                    point.shadow_enabled,
                );
            }
            LightType::Spot(spot) => {
                check_direction(&mut problems, "light_type.Spot.direction", spot.direction);
                check_range(
                    &mut problems,
                    "light_type.Spot.range",
                    spot.range,
                    spot.shadow_enabled,
                );
                let outer = spot.outer_cone_angle;
                let outer_usable = outer.is_finite() && outer > 0.0 && outer < FRAC_PI_2;
                if !outer_usable {
                    problems.push(error(
                        "light_type.Spot.outer_cone_angle",
                        format!(
                            "outer cone angle must be between 0 and π/2 radians (it is {outer})"
                        ),
                    ));
                }
                let inner = spot.inner_cone_angle;
                // Against a broken outer cone, only the inner cone's own
                // values are judged.
                let ceiling = if outer_usable { outer } else { f32::INFINITY };
                if !(inner.is_finite() && inner >= 0.0 && inner <= ceiling) {
                    problems.push(error(
                        "light_type.Spot.inner_cone_angle",
                        format!(
                            "inner cone angle must be between 0 and the outer cone angle {outer} (it is {inner})"
                        ),
                    ));
                }
                // A cone so narrow its projection overflows lights, but casts
                // no shadow.
                let range_casts = spot.range.is_finite() && spot.range > Mat4::SHADOW_NEAR;
                if spot.shadow_enabled
                    && outer_usable
                    && range_casts
                    && Mat4::try_perspective_rh_zo(outer * 2.0, 1.0, Mat4::SHADOW_NEAR, spot.range)
                        .is_none()
                {
                    problems.push(FieldProblem {
                        field: "light_type.Spot.outer_cone_angle",
                        severity: ProblemSeverity::Warning,
                        message: format!(
                            "casts no shadow: the cone is too narrow to project (it is {outer})"
                        ),
                    });
                }
            }
        }
        let (intensity_field, colour_field, intensity, colour) = match &self.light_type {
            LightType::Directional(sun) => (
                "light_type.Directional.intensity",
                "light_type.Directional.color",
                sun.intensity,
                sun.color,
            ),
            LightType::Point(point) => (
                "light_type.Point.intensity",
                "light_type.Point.color",
                point.intensity,
                point.color,
            ),
            LightType::Spot(spot) => (
                "light_type.Spot.intensity",
                "light_type.Spot.color",
                spot.intensity,
                spot.color,
            ),
        };
        let (bias_field, normal_bias_field, bias, normal_bias) = match &self.light_type {
            LightType::Directional(sun) => (
                "light_type.Directional.shadow_bias",
                "light_type.Directional.shadow_normal_bias",
                sun.shadow_bias,
                sun.shadow_normal_bias,
            ),
            LightType::Point(point) => (
                "light_type.Point.shadow_bias",
                "light_type.Point.shadow_normal_bias",
                point.shadow_bias,
                point.shadow_normal_bias,
            ),
            LightType::Spot(spot) => (
                "light_type.Spot.shadow_bias",
                "light_type.Spot.shadow_normal_bias",
                spot.shadow_bias,
                spot.shadow_normal_bias,
            ),
        };
        for (field, value) in [(bias_field, bias), (normal_bias_field, normal_bias)] {
            if !value.is_finite() {
                problems.push(error(
                    field,
                    format!("a shadow bias must be a finite number (it is {value})"),
                ));
            }
        }
        if !(intensity.is_finite() && intensity >= 0.0) {
            problems.push(error(
                intensity_field,
                format!("intensity must be a finite number, 0 or more (it is {intensity})"),
            ));
        }
        let channels = [colour.r, colour.g, colour.b];
        let channels_usable = channels.iter().all(|c| c.is_finite() && *c >= 0.0);
        if !channels_usable {
            problems.push(error(
                colour_field,
                format!(
                    "colour channels must be finite numbers, 0 or more (it is {} {} {})",
                    colour.r, colour.g, colour.b
                ),
            ));
        }
        // What the shader lights with is colour × intensity: each usable on its
        // own, their product may still overflow.
        if channels_usable
            && intensity.is_finite()
            && !channels.iter().all(|c| (c * intensity).is_finite())
        {
            problems.push(error(
                intensity_field,
                format!(
                    "intensity {intensity} times colour {} {} {} is too bright to compute",
                    colour.r, colour.g, colour.b
                ),
            ));
        }
        problems
    }
}

/// The rule of a direction at `field`: finite and not zero. A tiny one
/// still points (the engine scales it), however small its squared length.
fn check_direction(problems: &mut Vec<FieldProblem>, field: &'static str, d: Vec3) {
    let finite = d.x.is_finite() && d.y.is_finite() && d.z.is_finite();
    if !finite || (d.x == 0.0 && d.y == 0.0 && d.z == 0.0) {
        problems.push(error(
            field,
            format!("direction must be a finite, non-zero vector (it is {d:?})"),
        ));
    }
}

/// The range rules of a point or spot light at `field`.
fn check_range(problems: &mut Vec<FieldProblem>, field: &'static str, range: f32, shadow: bool) {
    if !range.is_finite() {
        problems.push(error(
            field,
            format!("range must be a finite number (it is {range})"),
        ));
    } else if range <= 0.0 {
        problems.push(error(
            field,
            format!("range must be greater than 0 (it is {range})"),
        ));
    } else if shadow && range <= Mat4::SHADOW_NEAR {
        problems.push(FieldProblem {
            field,
            severity: ProblemSeverity::Warning,
            message: format!(
                "casts no shadow: range must exceed {} (it is {range})",
                Mat4::SHADOW_NEAR
            ),
        });
    }
}

fn error(field: &'static str, message: String) -> FieldProblem {
    FieldProblem {
        field,
        severity: ProblemSeverity::Error,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_light_default() {
        let light = Light::default();
        assert!(light.enabled);
        assert!(matches!(light.light_type, LightType::Directional(_)));
    }

    #[test]
    fn test_light_directional() {
        let light = Light::directional();
        assert!(light.enabled);
        assert!(matches!(light.light_type, LightType::Directional(_)));
    }

    #[test]
    fn test_light_point() {
        let light = Light::point();
        assert!(light.enabled);
        assert!(matches!(light.light_type, LightType::Point(_)));
    }

    #[test]
    fn test_light_spot() {
        let light = Light::spot();
        assert!(light.enabled);
        assert!(matches!(light.light_type, LightType::Spot(_)));
    }

    #[test]
    fn test_light_enabled_toggle() {
        let mut light = Light::default();
        assert!(light.enabled);

        light.enabled = false;
        assert!(!light.enabled);
    }

    // --- A light checks its own values -------------------------------------

    use crate::ecs::World;
    use crate::scene::registration_for;
    use khora_core::math::{Vec3, FRAC_PI_2, PI};
    use khora_core::problems::ProblemSeverity;
    use khora_core::renderer::light::{DirectionalLight, PointLight, SpotLight};
    use std::any::TypeId;

    const POINT_RANGE: &str = "light_type.Point.range";
    const SPOT_RANGE: &str = "light_type.Spot.range";
    const SPOT_OUTER: &str = "light_type.Spot.outer_cone_angle";
    const SPOT_INNER: &str = "light_type.Spot.inner_cone_angle";
    const DIRECTIONAL_DIRECTION: &str = "light_type.Directional.direction";

    fn point(range: f32, shadow_enabled: bool) -> Light {
        Light::new(LightType::Point(PointLight {
            range,
            shadow_enabled,
            ..PointLight::default()
        }))
    }

    fn spot(range: f32, shadow_enabled: bool) -> Light {
        Light::new(LightType::Spot(SpotLight {
            range,
            shadow_enabled,
            ..SpotLight::default()
        }))
    }

    fn spot_cone(inner_cone_angle: f32, outer_cone_angle: f32) -> Light {
        Light::new(LightType::Spot(SpotLight {
            inner_cone_angle,
            outer_cone_angle,
            ..SpotLight::default()
        }))
    }

    fn directional(direction: Vec3) -> Light {
        Light::new(LightType::Directional(DirectionalLight {
            direction,
            ..DirectionalLight::default()
        }))
    }

    /// `light` has exactly one problem: on `field`, of `severity`.
    fn assert_one_problem(light: &Light, field: &str, severity: ProblemSeverity) -> FieldProblem {
        let check = light.check();
        assert_eq!(check.len(), 1, "check of {light:?}: {check:?}");
        assert_eq!(check[0].field, field, "field of {light:?}");
        assert_eq!(check[0].severity, severity, "severity of {light:?}");
        check[0].clone()
    }

    /// `light` has, among its problems, an `Error` on `field`.
    fn assert_error_on(light: &Light, field: &str) {
        let check = light.check();
        assert!(
            check
                .iter()
                .any(|p| p.field == field && p.severity == ProblemSeverity::Error),
            "no error on `{field}` for {light:?}: {check:?}"
        );
    }

    /// The lights a scene starts from — each kind's default, with or without
    /// shadows, disabled or not — have no problem.
    #[test]
    fn default_lights_have_no_problem() {
        let mut disabled = Light::spot();
        disabled.enabled = false;
        for light in [
            Light::default(),
            Light::directional(),
            Light::point(),
            Light::spot(),
            point(PointLight::default().range, true),
            spot(SpotLight::default().range, true),
            Light::new(LightType::Directional(DirectionalLight {
                shadow_enabled: true,
                ..DirectionalLight::default()
            })),
            disabled,
        ] {
            assert!(light.check().is_empty(), "{light:?}: {:?}", light.check());
        }
    }

    /// Ordinary values off the defaults are fine too: a range just past the
    /// shadow near plane with shadows on, a tiny range without shadows, a
    /// cone whose inner angle is 0 or equals the outer, a sun direction that
    /// is not normalized.
    #[test]
    fn usable_lights_off_the_defaults_have_no_problem() {
        for light in [
            point(0.2, true),
            spot(0.2, true),
            point(0.01, false),
            spot(0.01, false),
            spot_cone(0.0, 0.5),
            spot_cone(0.5, 0.5),
            spot_cone(0.1, FRAC_PI_2 - 0.01),
            directional(Vec3::new(0.0, -2.0, 0.0)),
            directional(Vec3::new(1e-3, 0.0, 0.0)),
        ] {
            assert!(light.check().is_empty(), "{light:?}: {:?}", light.check());
        }
    }

    /// A point or spot light whose range is 0 or negative reaches nothing:
    /// one `Error` on its range, naming the value — with or without shadows
    /// (no extra shadow warning for a range that is not positive).
    #[test]
    fn a_zero_or_negative_range_is_an_error() {
        for (range, shown) in [(0.0, "0"), (-0.0, "0"), (-1.0, "-1"), (-25.5, "-25.5")] {
            for shadows in [false, true] {
                for (light, field) in [
                    (point(range, shadows), POINT_RANGE),
                    (spot(range, shadows), SPOT_RANGE),
                ] {
                    let problem = assert_one_problem(&light, field, ProblemSeverity::Error);
                    assert!(
                        problem.message.contains(shown),
                        "the message names {range}: {}",
                        problem.message
                    );
                }
            }
        }
    }

    /// A range that is not a number, or is infinite, is an `Error` on it.
    #[test]
    fn a_non_finite_range_is_an_error() {
        for range in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            for shadows in [false, true] {
                assert_one_problem(&point(range, shadows), POINT_RANGE, ProblemSeverity::Error);
                assert_one_problem(&spot(range, shadows), SPOT_RANGE, ProblemSeverity::Error);
            }
        }
    }

    /// A shadow-casting light whose range does not reach past the shadow
    /// near plane (0.1) still lights, but casts no shadow: one `Warning` on
    /// its range that says so. The same light without shadows is fine.
    #[test]
    fn a_small_shadow_casting_range_is_a_warning() {
        for range in [0.05, 0.1, 1e-6] {
            for (light, field) in [
                (spot(range, true), SPOT_RANGE),
                (point(range, true), POINT_RANGE),
            ] {
                let problem = assert_one_problem(&light, field, ProblemSeverity::Warning);
                assert!(
                    problem.message.contains("shadow") && problem.message.contains("0.1"),
                    "the warning says the light casts no shadow below 0.1: {}",
                    problem.message
                );
            }
            assert!(
                spot(range, false).check().is_empty(),
                "spot {range} without shadows"
            );
            assert!(
                point(range, false).check().is_empty(),
                "point {range} without shadows"
            );
        }
    }

    /// A spot cone half-angle of 90° or more opens a half-space the spot's
    /// perspective cannot hold: an `Error` on the outer angle. So is a cone
    /// that is closed, reversed or not a number.
    #[test]
    fn a_spot_cone_of_ninety_degrees_or_more_is_an_error() {
        for outer in [FRAC_PI_2, FRAC_PI_2 + 0.01, 2.0, PI] {
            assert_one_problem(&spot_cone(0.1, outer), SPOT_OUTER, ProblemSeverity::Error);
        }
        for outer in [0.0, -0.1, f32::NAN, f32::INFINITY] {
            assert_error_on(&spot_cone(0.0, outer), SPOT_OUTER);
        }
    }

    /// An inner cone angle outside `[0, outer]` is an `Error` on the inner
    /// angle; the outer angle, usable, has none.
    #[test]
    fn an_inner_cone_outside_the_outer_is_an_error() {
        for (inner, outer) in [(0.6, 0.5), (-0.1, 0.5), (f32::NAN, 0.5), (1.0, 0.2)] {
            assert_one_problem(&spot_cone(inner, outer), SPOT_INNER, ProblemSeverity::Error);
        }
    }

    /// A sun direction of zero, or holding a NaN or an infinity, points
    /// nowhere: an `Error` on it.
    #[test]
    fn a_zero_or_non_finite_sun_direction_is_an_error() {
        for direction in [
            Vec3::ZERO,
            Vec3::new(f32::NAN, -1.0, 0.0),
            Vec3::new(0.0, f32::INFINITY, 0.0),
            Vec3::new(0.0, -1.0, f32::NEG_INFINITY),
        ] {
            assert_one_problem(
                &directional(direction),
                DIRECTIONAL_DIRECTION,
                ProblemSeverity::Error,
            );
        }
    }

    /// A sun direction that is tiny but not zero still points somewhere (the
    /// engine makes it unit): no problem, even where its squared length
    /// underflows to 0.
    #[test]
    fn a_tiny_non_zero_sun_direction_is_no_problem() {
        for direction in [Vec3::new(1e-30, 0.0, 0.0), Vec3::new(0.0, -1e-25, 1e-25)] {
            let light = directional(direction);
            assert!(light.check().is_empty(), "{light:?}: {:?}", light.check());
        }
    }

    /// A spot direction of zero, or holding a NaN or an infinity, points
    /// nowhere — as a sun's does: an `Error` on it.
    #[test]
    fn a_zero_or_non_finite_spot_direction_is_an_error() {
        for direction in [
            Vec3::ZERO,
            Vec3::new(f32::NAN, -1.0, 0.0),
            Vec3::new(0.0, f32::INFINITY, 0.0),
        ] {
            let light = Light::new(LightType::Spot(SpotLight {
                direction,
                ..SpotLight::default()
            }));
            assert_one_problem(&light, "light_type.Spot.direction", ProblemSeverity::Error);
        }
    }

    /// Every field a problem names is a path to a value in the light's
    /// serialized form — the JSON the inspector walks — so the inspector can
    /// mark the row it shows the value on.
    #[test]
    fn field_paths_match_the_serialized_form() {
        let registration = registration_for(TypeId::of::<Light>()).expect("Light registers itself");
        let broken = [
            point(0.0, false),
            point(0.05, true),
            spot(-1.0, false),
            spot(0.05, true),
            spot_cone(0.1, FRAC_PI_2),
            spot_cone(0.6, 0.5),
            directional(Vec3::ZERO),
            directional(Vec3::new(f32::NAN, 0.0, 0.0)),
        ];
        let mut seen = Vec::new();
        for light in broken {
            let mut world = World::new();
            let entity = world.spawn(light.clone());
            let json = (registration.to_json)(&world, entity).expect("a light serializes");
            let check = light.check();
            assert!(!check.is_empty(), "{light:?} has a problem");
            for problem in check {
                let mut value = &json;
                for key in problem.field.split('.') {
                    value = value.get(key).unwrap_or_else(|| {
                        panic!(
                            "`{}` of {light:?} does not resolve in its serialized form {json}",
                            problem.field
                        )
                    });
                }
                seen.push(problem.field);
            }
        }
        for field in [
            POINT_RANGE,
            SPOT_RANGE,
            SPOT_OUTER,
            SPOT_INNER,
            DIRECTIONAL_DIRECTION,
        ] {
            assert!(seen.contains(&field), "no problem on `{field}` was checked");
        }
    }

    // --- Intensity and colour ------------------------------------------------

    use khora_core::math::LinearRgba;

    /// A point light, a spot light and a sun, each of `intensity` and
    /// `color`, with the name of its variant in the serialized form.
    fn each_kind(intensity: f32, color: LinearRgba) -> [(Light, &'static str); 3] {
        [
            (
                Light::new(LightType::Point(PointLight {
                    intensity,
                    color,
                    ..PointLight::default()
                })),
                "Point",
            ),
            (
                Light::new(LightType::Spot(SpotLight {
                    intensity,
                    color,
                    ..SpotLight::default()
                })),
                "Spot",
            ),
            (
                Light::new(LightType::Directional(DirectionalLight {
                    intensity,
                    color,
                    ..DirectionalLight::default()
                })),
                "Directional",
            ),
        ]
    }

    /// An intensity that is not a number, infinite or negative is one `Error`
    /// on `light_type.<Variant>.intensity`, for every kind of light. An
    /// intensity of 0 (a light switched down to dark) is fine.
    #[test]
    fn a_non_finite_or_negative_intensity_is_an_error() {
        for intensity in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0] {
            for (light, variant) in each_kind(intensity, LinearRgba::WHITE) {
                assert_one_problem(
                    &light,
                    &format!("light_type.{variant}.intensity"),
                    ProblemSeverity::Error,
                );
            }
        }
        for (light, _) in each_kind(0.0, LinearRgba::WHITE) {
            assert!(light.check().is_empty(), "{light:?}: {:?}", light.check());
        }
    }

    /// A colour holding a channel that is not a number, infinite or negative
    /// is one `Error` on `light_type.<Variant>.color` — one for the colour,
    /// however many of its channels are broken. A black colour is fine.
    #[test]
    fn a_non_finite_or_negative_colour_is_an_error() {
        for color in [
            LinearRgba::new(f32::NAN, 1.0, 1.0, 1.0),
            LinearRgba::new(1.0, f32::INFINITY, 1.0, 1.0),
            LinearRgba::new(1.0, 1.0, -0.5, 1.0),
            LinearRgba::new(1.0, f32::NEG_INFINITY, 1.0, 1.0),
            LinearRgba::new(-1.0, f32::NAN, 0.0, 1.0),
        ] {
            for (light, variant) in each_kind(5.0, color) {
                assert_one_problem(
                    &light,
                    &format!("light_type.{variant}.color"),
                    ProblemSeverity::Error,
                );
            }
        }
        for (light, _) in each_kind(5.0, LinearRgba::new(0.0, 0.0, 0.0, 1.0)) {
            assert!(light.check().is_empty(), "{light:?}: {:?}", light.check());
        }
    }

    /// The intensity and colour problems name paths to values in the light's
    /// serialized form, as the other problems do.
    #[test]
    fn intensity_and_colour_paths_match_the_serialized_form() {
        let registration = registration_for(TypeId::of::<Light>()).expect("Light registers itself");
        let mut broken = Vec::new();
        broken.extend(each_kind(f32::NAN, LinearRgba::WHITE));
        broken.extend(each_kind(-1.0, LinearRgba::WHITE));
        broken.extend(each_kind(5.0, LinearRgba::new(-1.0, 1.0, 1.0, 1.0)));
        broken.extend(each_kind(
            5.0,
            LinearRgba::new(1.0, f32::INFINITY, 1.0, 1.0),
        ));
        let mut seen = Vec::new();
        for (light, _) in broken {
            let mut world = World::new();
            let entity = world.spawn(light.clone());
            let json = (registration.to_json)(&world, entity).expect("a light serializes");
            let check = light.check();
            assert!(!check.is_empty(), "{light:?} has a problem");
            for problem in check {
                let mut value = &json;
                for key in problem.field.split('.') {
                    value = value.get(key).unwrap_or_else(|| {
                        panic!(
                            "`{}` of {light:?} does not resolve in its serialized form {json}",
                            problem.field
                        )
                    });
                }
                seen.push(problem.field);
            }
        }
        for field in [
            "light_type.Point.intensity",
            "light_type.Spot.intensity",
            "light_type.Directional.intensity",
            "light_type.Point.color",
            "light_type.Spot.color",
            "light_type.Directional.color",
        ] {
            assert!(seen.contains(&field), "no problem on `{field}` was checked");
        }
    }
}
