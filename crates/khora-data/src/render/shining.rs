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

//! The lights that shine this frame — the single list every light consumer
//! (the render extraction, the shadow derivation, the IBL sky) reads.

use khora_core::math::Vec3;
use khora_core::problems::ProblemSeverity;
use khora_core::renderer::light::LightType;

use crate::ecs::{GlobalTransform, Light, World};

/// A light that shines: its authored values, and its world-space position
/// and direction.
#[derive(Debug, Clone, Copy)]
pub struct ShiningLight<'w> {
    /// The light's authored component.
    pub light: &'w Light,
    /// World-space position (finite).
    pub position: Vec3,
    /// World-space **unit** direction for a directional or spot light;
    /// `Vec3::ZERO` for a point light.
    pub direction: Vec3,
}

/// The lights that shine in `world`, in query order.
///
/// This is the single place that says which lights exist for a frame:
/// index `i` in the returned list is the light index every consumer shares
/// (`RenderWorld.lights[i]`, `ShadowView.matrices[&i]`).
///
/// A light shines when it is `enabled`, its [`Light::check`] holds no
/// `Error` (a `Warning` still shines), its world position is finite, and —
/// for a directional or spot light — its world direction
/// (`transform_direction` of the authored one) is finite and not zero. The
/// direction is published as a unit vector (divided by its largest absolute
/// component, then normalized, so a tiny direction still points). A point
/// light's direction is `Vec3::ZERO`.
pub fn shining_lights(world: &World) -> Vec<ShiningLight<'_>> {
    world
        .query::<(&Light, &GlobalTransform)>()
        .filter_map(|(light, global)| shining(light, global))
        .collect()
}

/// `light` placed by `global`, when it shines.
fn shining<'w>(light: &'w Light, global: &GlobalTransform) -> Option<ShiningLight<'w>> {
    if !light.enabled {
        return None;
    }
    // What the light's own check calls unusable stays out of the frame; its
    // problem is listed, its values kept.
    if light
        .check()
        .iter()
        .any(|problem| problem.severity == ProblemSeverity::Error)
    {
        return None;
    }
    let position = global.0.translation();
    if !is_finite(position) {
        return None;
    }
    let direction = match &light.light_type {
        LightType::Point(_) => Vec3::ZERO,
        LightType::Directional(sun) => unit(global.0.transform_direction(sun.direction))?,
        LightType::Spot(spot) => unit(global.0.transform_direction(spot.direction))?,
    };
    Some(ShiningLight {
        light,
        position,
        direction,
    })
}

/// `direction` as a unit vector, or `None` when it points nowhere (zero, or
/// holding a NaN or an infinity). Divided by its largest component first, so
/// a direction however small still points, where its squared length would
/// underflow.
fn unit(direction: Vec3) -> Option<Vec3> {
    if !is_finite(direction) {
        return None;
    }
    let largest = direction
        .x
        .abs()
        .max(direction.y.abs())
        .max(direction.z.abs());
    (largest > 0.0).then(|| (direction / largest).normalize())
}

fn is_finite(v: Vec3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::GlobalTransform;
    use khora_core::math::{LinearRgba, Mat4, Quaternion, FRAC_PI_2};
    use khora_core::problems::ProblemSeverity;
    use khora_core::renderer::light::{DirectionalLight, LightType, PointLight, SpotLight};

    /// Where the usable marker light of a test sits.
    const MARKER_AT: Vec3 = Vec3::new(7.0, 8.0, 9.0);

    fn point(edit: impl FnOnce(&mut PointLight)) -> Light {
        let mut point = PointLight::default();
        edit(&mut point);
        Light::new(LightType::Point(point))
    }

    fn spot(edit: impl FnOnce(&mut SpotLight)) -> Light {
        let mut spot = SpotLight::default();
        edit(&mut spot);
        Light::new(LightType::Spot(spot))
    }

    fn sun(edit: impl FnOnce(&mut DirectionalLight)) -> Light {
        let mut sun = DirectionalLight::default();
        edit(&mut sun);
        Light::new(LightType::Directional(sun))
    }

    fn disabled(mut light: Light) -> Light {
        light.enabled = false;
        light
    }

    fn has_error(light: &Light) -> bool {
        light
            .check()
            .iter()
            .any(|p| p.severity == ProblemSeverity::Error)
    }

    fn finite(v: Vec3) -> bool {
        v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
    }

    fn is_unit(v: Vec3) -> bool {
        finite(v) && (v.length() - 1.0).abs() < 1e-5
    }

    /// The authored direction of a sun or a spot; `None` for a point light.
    fn authored_direction(light: &Light) -> Option<Vec3> {
        match &light.light_type {
            LightType::Directional(d) => Some(d.direction),
            LightType::Spot(s) => Some(s.direction),
            LightType::Point(_) => None,
        }
    }

    /// The lights a scene author can break, one way each: every one holds an
    /// `Error` on its own values (range, cone, direction, intensity, colour).
    fn broken_lights() -> Vec<(&'static str, Light)> {
        vec![
            ("point range 0", point(|p| p.range = 0.0)),
            ("point range NaN", point(|p| p.range = f32::NAN)),
            ("spot range 0", spot(|s| s.range = 0.0)),
            ("spot range NaN", spot(|s| s.range = f32::NAN)),
            ("spot cone π/2", spot(|s| s.outer_cone_angle = FRAC_PI_2)),
            ("spot cone > π/2", spot(|s| s.outer_cone_angle = 2.0)),
            ("spot cone NaN", spot(|s| s.outer_cone_angle = f32::NAN)),
            ("spot direction zero", spot(|s| s.direction = Vec3::ZERO)),
            (
                "spot direction NaN",
                spot(|s| s.direction = Vec3::new(f32::NAN, -1.0, 0.0)),
            ),
            ("sun direction zero", sun(|d| d.direction = Vec3::ZERO)),
            (
                "sun direction NaN",
                sun(|d| d.direction = Vec3::new(0.0, f32::NAN, 0.0)),
            ),
            ("point intensity NaN", point(|p| p.intensity = f32::NAN)),
            ("sun intensity NaN", sun(|d| d.intensity = f32::NAN)),
            (
                "spot negative colour",
                spot(|s| s.color = LinearRgba::new(-1.0, 1.0, 1.0, 1.0)),
            ),
            (
                "sun negative colour",
                sun(|d| d.color = LinearRgba::new(1.0, -0.5, 1.0, 1.0)),
            ),
        ]
    }

    /// A world holding a usable point light at [`MARKER_AT`] and `light` at
    /// the origin, in that order.
    fn beside_a_marker(light: Light) -> World {
        let mut world = World::new();
        world.spawn((
            GlobalTransform::at_position(MARKER_AT),
            point(|p| p.range = 5.0),
        ));
        world.spawn((GlobalTransform::at_position(Vec3::ZERO), light));
        world
    }

    /// Only the marker shines in `world`.
    fn assert_only_the_marker_shines(world: &World, what: &str) {
        let shining = shining_lights(world);
        assert_eq!(
            shining.len(),
            1,
            "{what}: {} light(s) shine, only the marker should",
            shining.len()
        );
        assert_eq!(shining[0].position, MARKER_AT, "{what}: the marker shines");
    }

    /// A light whose own values hold an `Error` — a range of 0 or NaN, a
    /// cone of 90° or more or NaN, a zero or NaN direction, a NaN intensity,
    /// a negative colour — does not shine; the usable light beside it does.
    #[test]
    fn a_light_with_an_error_does_not_shine() {
        for (what, light) in broken_lights() {
            assert_only_the_marker_shines(&beside_a_marker(light), what);
        }
    }

    /// A shadow-casting spot light of range 0.05 is only warned about (it
    /// casts no shadow): it still shines, with its own values.
    #[test]
    fn a_warning_does_not_dim_a_light() {
        let warned = spot(|s| {
            s.range = 0.05;
            s.shadow_enabled = true;
        });
        let check = warned.check();
        assert!(
            !check.is_empty() && check.iter().all(|p| p.severity == ProblemSeverity::Warning),
            "the premise: only warnings: {check:?}"
        );

        let world = beside_a_marker(warned);
        let shining = shining_lights(&world);

        assert_eq!(shining.len(), 2, "the warned spot shines beside the marker");
        assert_eq!(shining[1].position, Vec3::ZERO);
        match &shining[1].light.light_type {
            LightType::Spot(s) => assert_eq!(s.range, 0.05, "its authored range"),
            other => panic!("the second light that shines is the spot: {other:?}"),
        }
    }

    /// A disabled light does not shine, however usable its values; the
    /// enabled lights around it do, in order.
    #[test]
    fn a_disabled_light_does_not_shine() {
        let mut world = World::new();
        world.spawn((
            GlobalTransform::at_position(Vec3::new(1.0, 0.0, 0.0)),
            sun(|_| {}),
        ));
        world.spawn((
            GlobalTransform::at_position(Vec3::new(2.0, 0.0, 0.0)),
            disabled(point(|_| {})),
        ));
        world.spawn((
            GlobalTransform::at_position(Vec3::new(3.0, 0.0, 0.0)),
            spot(|_| {}),
        ));
        world.spawn((
            GlobalTransform::at_position(Vec3::new(4.0, 0.0, 0.0)),
            disabled(spot(|_| {})),
        ));
        world.spawn((
            GlobalTransform::at_position(Vec3::new(5.0, 0.0, 0.0)),
            disabled(sun(|_| {})),
        ));

        let positions: Vec<Vec3> = shining_lights(&world).iter().map(|s| s.position).collect();

        assert_eq!(
            positions,
            vec![Vec3::new(1.0, 0.0, 0.0), Vec3::new(3.0, 0.0, 0.0)]
        );
    }

    /// A light whose own values are clean but whose entity sits at a NaN or
    /// infinite place does not shine — and its check stays clean: the
    /// transform's problem is not the light's.
    #[test]
    fn a_light_under_a_broken_transform_does_not_shine() {
        let broken_transforms = [
            (
                "NaN translation",
                GlobalTransform::at_position(Vec3::new(f32::NAN, 0.0, 0.0)),
            ),
            (
                "infinite translation",
                GlobalTransform::at_position(Vec3::new(0.0, f32::INFINITY, 0.0)),
            ),
            (
                "every entry NaN",
                GlobalTransform::new(
                    Mat4::from_scale(Vec3::new(f32::NAN, f32::NAN, f32::NAN))
                        * Mat4::from_translation(Vec3::new(f32::NAN, f32::NAN, f32::NAN)),
                ),
            ),
        ];
        for (what, transform) in broken_transforms {
            for light in [point(|_| {}), spot(|_| {}), sun(|_| {})] {
                assert!(
                    light.check().is_empty(),
                    "the premise: a clean light: {:?}",
                    light.check()
                );
                let mut world = World::new();
                world.spawn((
                    GlobalTransform::at_position(MARKER_AT),
                    point(|p| p.range = 5.0),
                ));
                world.spawn((transform, light.clone()));

                assert_only_the_marker_shines(&world, &format!("{what}, {:?}", light.light_type));
                assert!(
                    light.check().is_empty(),
                    "{what}: the light's own check stays clean"
                );
            }
        }
    }

    /// A sun or spot authored with a tiny direction still points: it is
    /// published as a unit vector — under the identity, and under a parent
    /// scaled down to 1e-20.
    #[test]
    fn a_shining_direction_is_unit_even_when_tiny() {
        let cases = [
            (
                "identity",
                GlobalTransform::identity(),
                Vec3::new(1e-30, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
            ),
            (
                "scaled parent",
                GlobalTransform::new(Mat4::from_scale(Vec3::new(1e-20, 1e-20, 1e-20))),
                Vec3::new(1e-30, 1e-30, 0.0),
                Vec3::new(1.0, 1.0, 0.0).normalize(),
            ),
            (
                "non-uniform parent",
                GlobalTransform::new(Mat4::from_scale(Vec3::new(4.0, 1.0, 1.0))),
                Vec3::new(1e-30, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
            ),
        ];
        for (what, transform, authored, expected) in cases {
            for light in [
                sun(|d| d.direction = authored),
                spot(|s| s.direction = authored),
            ] {
                let mut world = World::new();
                world.spawn((transform, light.clone()));

                let shining = shining_lights(&world);

                assert_eq!(shining.len(), 1, "{what}: the light shines: {light:?}");
                let direction = shining[0].direction;
                assert!(
                    is_unit(direction),
                    "{what}: {direction:?} is not a unit vector"
                );
                assert!(
                    (direction - expected).length() < 1e-5,
                    "{what}: {direction:?} where {expected:?} was expected"
                );
            }
        }
    }

    /// Swept over usable and broken lights, enabled or not, under usable and
    /// broken transforms: a light shines exactly when it is enabled, its
    /// check holds no `Error`, and its world position — and, for a sun or a
    /// spot, its world direction — are finite (the direction not zero).
    /// Every value a shining light publishes is finite; a sun's or a spot's
    /// direction is a unit vector, a point light's is zero.
    #[test]
    fn shines_exactly_when_enabled_clean_and_finite() {
        let mut lights: Vec<Light> = broken_lights().into_iter().map(|(_, l)| l).collect();
        lights.extend([
            point(|_| {}),
            spot(|_| {}),
            sun(|_| {}),
            spot(|s| {
                s.range = 0.05;
                s.shadow_enabled = true;
            }),
            spot(|s| s.inner_cone_angle = s.outer_cone_angle),
            sun(|d| d.direction = Vec3::new(1e-30, 0.0, 0.0)),
            point(|p| p.intensity = 0.0),
            point(|p| p.intensity = f32::INFINITY),
            spot(|s| s.intensity = -1.0),
            point(|p| p.color = LinearRgba::new(0.0, 0.0, 0.0, 1.0)),
            point(|p| p.color = LinearRgba::new(f32::INFINITY, 1.0, 1.0, 1.0)),
        ]);
        let disabled_copies: Vec<Light> = lights.iter().cloned().map(disabled).collect();
        lights.extend(disabled_copies);

        let turned = Quaternion::from_axis_angle(Vec3::new(1.0, 2.0, 0.5).normalize(), 0.7);
        let transforms = [
            GlobalTransform::identity(),
            GlobalTransform::at_position(Vec3::new(3.0, -2.0, 1.0)),
            GlobalTransform::new(
                Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0))
                    * Mat4::from_quat(turned)
                    * Mat4::from_scale(Vec3::new(1.0, 3.0, 0.5)),
            ),
            GlobalTransform::new(Mat4::from_scale(Vec3::new(1e-20, 1e-20, 1e-20))),
            GlobalTransform::new(Mat4::from_scale(Vec3::ZERO)),
            GlobalTransform::at_position(Vec3::new(f32::NAN, 0.0, 0.0)),
            GlobalTransform::at_position(Vec3::new(0.0, 0.0, f32::NEG_INFINITY)),
        ];

        let mut disagreements = Vec::new();
        for light in &lights {
            for transform in transforms {
                let position = transform.0.translation();
                let direction_finite = authored_direction(light).is_none_or(|authored| {
                    let world_direction = transform.0.transform_direction(authored);
                    finite(world_direction) && world_direction != Vec3::ZERO
                });
                let expected =
                    light.enabled && !has_error(light) && finite(position) && direction_finite;

                let mut world = World::new();
                world.spawn((transform, light.clone()));
                let shining = shining_lights(&world);

                if expected != (shining.len() == 1) {
                    disagreements.push(format!(
                        "expected to shine: {expected}, shines: {} — {light:?} under {:?}",
                        shining.len(),
                        transform.0
                    ));
                    continue;
                }
                for shone in &shining {
                    let mut published = vec![shone.position, shone.direction];
                    let (intensity, color) = match &shone.light.light_type {
                        LightType::Directional(d) => (d.intensity, d.color),
                        LightType::Point(p) => {
                            published.push(Vec3::new(p.range, 0.0, 0.0));
                            (p.intensity, p.color)
                        }
                        LightType::Spot(s) => {
                            published.push(Vec3::new(
                                s.range,
                                s.inner_cone_angle,
                                s.outer_cone_angle,
                            ));
                            (s.intensity, s.color)
                        }
                    };
                    published.push(Vec3::new(intensity, color.r, color.g));
                    published.push(Vec3::new(color.b, 0.0, 0.0));
                    if !published.iter().all(|v| finite(*v)) {
                        disagreements.push(format!(
                            "a non-finite value was published: {published:?} — {light:?}"
                        ));
                    }
                    let direction_ok = match shone.light.light_type {
                        LightType::Point(_) => shone.direction == Vec3::ZERO,
                        _ => is_unit(shone.direction),
                    };
                    if !direction_ok {
                        disagreements.push(format!(
                            "direction {:?} of {light:?} under {:?}",
                            shone.direction, transform.0
                        ));
                    }
                }
            }
        }
        assert!(disagreements.is_empty(), "{}", disagreements.join("\n"));
    }
}
