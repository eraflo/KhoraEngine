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

//! Gizmo manipulation — grabbing a handle and turning cursor motion into a
//! transform delta.
//!
//! No ECS, no rendering, no input plumbing: the editor supplies a cursor
//! [`Ray`] plus the world-space frame of the manipulator, and this module
//! answers *which handle is under the cursor* and *how far it has been
//! dragged*. That keeps the part most likely to be subtly wrong — the
//! manipulation semantics — testable without a window. The intersection math
//! underneath belongs to [`Ray`], not here.
//!
//! Handles are **world-aligned for Move and Rotate, object-aligned for
//! Scale**. That is the convention every DCC settled on, and not arbitrarily:
//! a world-aligned scale handle on a rotated object would have to shear it to
//! mean anything, which a `Transform` cannot represent.
//!
//! Deltas are expressed relative to the transform each entity had when the drag
//! began ([`GizmoDelta::apply`]) rather than accumulated frame by frame, so a
//! long drag cannot drift away from where the cursor says it should be.

use super::state::GizmoMode;
use crate::math::{Quaternion, Ray, Vec3};

/// How close the cursor ray must come to a handle to grab it, as a fraction of
/// the gizmo's world size. Generous on purpose — a handle you have to hit
/// pixel-perfectly is a handle you fight.
const PICK_TOLERANCE: f32 = 0.14;

/// Floor on the gizmo size used as a scale drag's unit, so a degenerate size
/// cannot divide the gesture into infinity.
const MIN_GIZMO_SIZE: f32 = 1e-6;

/// Smallest scale factor a drag may produce. Zero collapses the entity onto a
/// plane it can never be dragged back out of; negative turns it inside out.
const MIN_SCALE_FACTOR: f32 = 0.01;

/// One of the three manipulator handles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GizmoAxis {
    /// First handle — red.
    X,
    /// Second handle — green.
    Y,
    /// Third handle — blue.
    Z,
}

impl GizmoAxis {
    /// All three handles, in draw and hit-test order.
    pub const ALL: [Self; 3] = [Self::X, Self::Y, Self::Z];

    /// Index of this axis into a [`GizmoBasis`].
    pub const fn index(self) -> usize {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Z => 2,
        }
    }

    /// The colour this handle is drawn in.
    ///
    /// Defined once, here, so the geometry generator and anything that
    /// highlights the grabbed handle cannot disagree about which axis is which.
    pub const fn color(self) -> [f32; 4] {
        match self {
            Self::X => [0.95, 0.32, 0.28, 1.0],
            Self::Y => [0.34, 0.88, 0.43, 1.0],
            Self::Z => [0.35, 0.63, 0.97, 1.0],
        }
    }
}

/// World-space directions of the three handles, indexed by [`GizmoAxis::index`].
pub type GizmoBasis = [Vec3; 3];

/// The frame the handles are drawn and dragged in for `mode`.
///
/// Object-aligned for [`GizmoMode::Scale`] (see the module docs), world-aligned
/// otherwise.
pub fn gizmo_basis(mode: GizmoMode, rotation: Quaternion) -> GizmoBasis {
    if mode == GizmoMode::Scale {
        [
            (rotation * Vec3::X).normalize(),
            (rotation * Vec3::Y).normalize(),
            (rotation * Vec3::Z).normalize(),
        ]
    } else {
        [Vec3::X, Vec3::Y, Vec3::Z]
    }
}

/// The transform an entity had when a drag began.
///
/// Deliberately not `Transform` — that lives in `khora-data`, which sits above
/// this crate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GizmoTransform {
    /// Position.
    pub translation: Vec3,
    /// Orientation.
    pub rotation: Quaternion,
    /// Per-axis scale.
    pub scale: Vec3,
}

/// What a drag has done so far, in world space, relative to the drag's start.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GizmoDelta {
    /// Move every dragged entity by this world-space offset.
    Translate(Vec3),
    /// Turn every dragged entity by `angle` radians about `axis` through `pivot`.
    Rotate {
        /// Normalised world-space rotation axis.
        axis: Vec3,
        /// Signed angle, in radians. Unbounded — a drag may wind past a turn.
        angle: f32,
        /// World-space centre of rotation.
        pivot: Vec3,
    },
    /// Multiply every dragged entity's scale by these per-axis factors.
    Scale {
        /// Per-axis multipliers, in the object's own frame.
        factors: Vec3,
        /// World-space centre the scaling radiates from.
        pivot: Vec3,
    },
}

impl GizmoDelta {
    /// Applies this delta to the transform an entity had when the drag began.
    ///
    /// Taking the *start* transform rather than the current one is what makes a
    /// drag reversible: dragging out and back lands exactly where it started,
    /// with no accumulated floating-point residue.
    pub fn apply(&self, start: GizmoTransform) -> GizmoTransform {
        match *self {
            Self::Translate(offset) => GizmoTransform {
                translation: start.translation + offset,
                ..start
            },
            Self::Rotate { axis, angle, pivot } => {
                let turn = Quaternion::from_axis_angle(axis, angle);
                GizmoTransform {
                    // Orbiting the pivot, not just spinning in place: with one
                    // entity selected the pivot *is* its origin so the two are
                    // the same, but with several the selection has to rotate as
                    // one body.
                    translation: pivot + turn * (start.translation - pivot),
                    rotation: (turn * start.rotation).normalize(),
                    scale: start.scale,
                }
            }
            Self::Scale { factors, pivot } => {
                let offset = start.translation - pivot;
                GizmoTransform {
                    translation: pivot
                        + Vec3::new(
                            offset.x * factors.x,
                            offset.y * factors.y,
                            offset.z * factors.z,
                        ),
                    rotation: start.rotation,
                    scale: Vec3::new(
                        start.scale.x * factors.x,
                        start.scale.y * factors.y,
                        start.scale.z * factors.z,
                    ),
                }
            }
        }
    }
}

/// A manipulation in progress: one grabbed handle, tracked across frames.
#[derive(Debug, Clone, Copy)]
pub struct GizmoDrag {
    /// The grabbed handle.
    pub axis: GizmoAxis,
    /// The tool the drag was started with.
    pub mode: GizmoMode,
    /// World-space direction of the grabbed handle, captured at grab time so a
    /// rotating object cannot move the axis out from under the cursor.
    pub direction: Vec3,
    /// World-space centre of the manipulator.
    pub pivot: Vec3,
    /// The gizmo's world size at grab time — the unit a scale drag is measured
    /// in, so dragging a handle out to twice its length doubles the scale.
    size: f32,
    /// Parameter under the cursor when the handle was grabbed.
    grab: f32,
    /// Parameter under the cursor on the previous update. Rotation only.
    previous: f32,
    /// Accumulated rotation, in radians. Rotation only.
    ///
    /// Summed from per-update increments rather than measured against `grab`
    /// because the raw angle wraps at ±π: measured absolutely, a drag past half
    /// a turn would snap the object back the other way.
    winding: f32,
}

impl GizmoDrag {
    /// Grabs `axis` at the cursor ray, if the ray yields a usable parameter.
    pub fn begin(
        mode: GizmoMode,
        axis: GizmoAxis,
        pivot: Vec3,
        basis: &GizmoBasis,
        size: f32,
        ray: &Ray,
    ) -> Option<Self> {
        if mode == GizmoMode::Select {
            return None;
        }
        let direction = basis[axis.index()];
        let grab = drag_parameter(mode, pivot, direction, ray)?;
        Some(Self {
            axis,
            mode,
            direction,
            pivot,
            size,
            grab,
            previous: grab,
            winding: 0.0,
        })
    }

    /// Advances the drag to a new cursor ray.
    ///
    /// Returns `None` when the ray no longer yields a usable parameter — the
    /// cursor is edge-on to the rotation plane, or parallel to the axis. The
    /// caller should hold the last delta rather than snapping the entity.
    pub fn update(&mut self, ray: &Ray) -> Option<GizmoDelta> {
        let now = drag_parameter(self.mode, self.pivot, self.direction, ray)?;

        match self.mode {
            GizmoMode::Move => Some(GizmoDelta::Translate(self.direction * (now - self.grab))),
            GizmoMode::Rotate => {
                self.winding += wrap_signed(now - self.previous);
                self.previous = now;
                Some(GizmoDelta::Rotate {
                    axis: self.direction,
                    angle: self.winding,
                    pivot: self.pivot,
                })
            }
            GizmoMode::Scale => {
                // Measured against the gizmo's own size so the gesture reads
                // the same whatever the zoom level: drag the handle out to
                // twice its drawn length, get twice the scale.
                let factor =
                    (1.0 + (now - self.grab) / self.size.max(MIN_GIZMO_SIZE)).max(MIN_SCALE_FACTOR);
                let mut factors = Vec3::new(1.0, 1.0, 1.0);
                match self.axis {
                    GizmoAxis::X => factors.x = factor,
                    GizmoAxis::Y => factors.y = factor,
                    GizmoAxis::Z => factors.z = factor,
                }
                Some(GizmoDelta::Scale {
                    factors,
                    pivot: self.pivot,
                })
            }
            GizmoMode::Select => None,
        }
    }
}

/// The nearest handle whose geometry the ray passes within tolerance of, or
/// `None` when the cursor is not over the manipulator.
///
/// Nearest *along the ray*, so a handle in front wins over one behind it —
/// which is what "the one you can see" means when two overlap on screen.
pub fn pick_handle(
    mode: GizmoMode,
    pivot: Vec3,
    basis: &GizmoBasis,
    size: f32,
    ray: &Ray,
) -> Option<GizmoAxis> {
    if mode == GizmoMode::Select {
        return None;
    }
    let tolerance = size * PICK_TOLERANCE;
    let mut best: Option<(f32, GizmoAxis)> = None;

    for axis in GizmoAxis::ALL {
        let direction = basis[axis.index()];
        let proximity = match mode {
            GizmoMode::Rotate => ring_proximity(pivot, direction, size, ray),
            _ => arm_proximity(pivot, direction, size, ray),
        };
        let Some((along_ray, miss)) = proximity else {
            continue;
        };
        if miss > tolerance {
            continue;
        }
        if best.is_none_or(|(nearest, _)| along_ray < nearest) {
            best = Some((along_ray, axis));
        }
    }

    best.map(|(_, axis)| axis)
}

/// The scalar the drag is measured in: distance along the axis for Move and
/// Scale, angle around it for Rotate.
fn drag_parameter(mode: GizmoMode, pivot: Vec3, direction: Vec3, ray: &Ray) -> Option<f32> {
    match mode {
        GizmoMode::Rotate => {
            let point = ray.at(ray.intersect_plane(pivot, direction)?);
            let (u, v) = perpendiculars(direction);
            let offset = point - pivot;
            Some(offset.dot(v).atan2(offset.dot(u)))
        }
        _ => ray.closest_param_on_line(pivot, direction),
    }
}

/// A stable pair of unit axes perpendicular to `axis` and to each other.
///
/// Any perpendicular pair would do for measuring an *angular difference* or for
/// sweeping a ring; what matters is that the same `axis` always yields the same
/// pair, so neither the measured angle nor the drawn geometry jitters between
/// frames.
pub fn perpendiculars(axis: Vec3) -> (Vec3, Vec3) {
    // Seed with the world axis least aligned with `axis`, so the cross product
    // is never near-degenerate.
    let seed = if axis.x.abs() < axis.y.abs() && axis.x.abs() < axis.z.abs() {
        Vec3::X
    } else if axis.y.abs() < axis.z.abs() {
        Vec3::Y
    } else {
        Vec3::Z
    };
    let u = axis.cross(seed).normalize();
    (u, axis.cross(u))
}

/// How near the ray passes to a translate/scale arm: distance along the ray,
/// and how far it misses by.
fn arm_proximity(pivot: Vec3, direction: Vec3, size: f32, ray: &Ray) -> Option<(f32, f32)> {
    // The segment, not the line it lies on: the arm stops at `size`, so the
    // empty space beyond its tip must not be grabbable out to infinity.
    let point = ray.closest_point_on_segment(pivot, pivot + direction * size);
    let along_ray = (point - ray.origin).dot(ray.direction);
    if along_ray < 0.0 {
        return None;
    }
    Some((along_ray, ray.distance_to_point(point)))
}

/// How near the ray passes to a rotation ring: distance along the ray, and how
/// far the hit lands from the ring itself.
fn ring_proximity(pivot: Vec3, axis: Vec3, size: f32, ray: &Ray) -> Option<(f32, f32)> {
    let along_ray = ray.intersect_plane(pivot, axis)?;
    let miss = ((ray.at(along_ray) - pivot).length() - size).abs();
    Some((along_ray, miss))
}

/// Folds an angle into `(-π, π]`.
fn wrap_signed(angle: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let folded = (angle + PI).rem_euclid(TAU) - PI;
    // `rem_euclid` maps exactly -π to -π; the half-open convention wants π.
    if folded <= -PI {
        PI
    } else {
        folded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORLD: GizmoBasis = [Vec3::X, Vec3::Y, Vec3::Z];

    /// A ray straight down -Z from `(x, y, 5)`.
    fn from_front(x: f32, y: f32) -> Ray {
        Ray::new(Vec3::new(x, y, 5.0), Vec3::new(0.0, 0.0, -1.0))
    }

    /// A ray straight down -Y from `(x, 5, z)` — used for the Y rotation ring,
    /// which lies in the XZ plane.
    fn from_above(x: f32, z: f32) -> Ray {
        Ray::new(Vec3::new(x, 5.0, z), Vec3::new(0.0, -1.0, 0.0))
    }

    fn identity() -> GizmoTransform {
        GizmoTransform {
            translation: Vec3::ZERO,
            rotation: Quaternion::IDENTITY,
            scale: Vec3::new(1.0, 1.0, 1.0),
        }
    }

    /// A ray aimed at the middle of the X arm grabs X, not one of the axes it
    /// happens to pass near the origin of.
    #[test]
    fn ray_over_an_arm_picks_that_arm() {
        let hit = pick_handle(GizmoMode::Move, Vec3::ZERO, &WORLD, 1.0, &from_front(0.5, 0.0));
        assert_eq!(hit, Some(GizmoAxis::X));
    }

    /// A ray well clear of every arm grabs nothing — the tolerance is generous,
    /// not unbounded.
    #[test]
    fn ray_clear_of_the_gizmo_picks_nothing() {
        let hit = pick_handle(GizmoMode::Move, Vec3::ZERO, &WORLD, 1.0, &from_front(0.5, 0.5));
        assert_eq!(hit, None);
    }

    /// The arm is a segment, not the infinite line it lies on: aiming well past
    /// its tip must miss.
    #[test]
    fn ray_past_the_arm_tip_picks_nothing() {
        let hit = pick_handle(GizmoMode::Move, Vec3::ZERO, &WORLD, 1.0, &from_front(4.0, 0.0));
        assert_eq!(hit, None);
    }

    /// The pointer tool has no handles, so nothing over the manipulator is
    /// grabbable and a click falls through to entity picking.
    #[test]
    fn select_mode_has_no_handles() {
        let hit = pick_handle(
            GizmoMode::Select,
            Vec3::ZERO,
            &WORLD,
            1.0,
            &from_front(0.5, 0.0),
        );
        assert_eq!(hit, None);
    }

    /// Dragging the X handle sideways moves along X only, by the distance the
    /// cursor travelled.
    #[test]
    fn dragging_an_arm_translates_along_it() {
        let mut drag = GizmoDrag::begin(
            GizmoMode::Move,
            GizmoAxis::X,
            Vec3::ZERO,
            &WORLD,
            1.0,
            &from_front(0.0, 0.0),
        )
        .expect("the ray is perpendicular to X, so the grab resolves");

        let delta = drag.update(&from_front(2.0, 0.0)).expect("still perpendicular");

        let GizmoDelta::Translate(offset) = delta else {
            panic!("Move must yield a translation, got {delta:?}");
        };
        assert!((offset.x - 2.0).abs() < 1e-4, "moved {offset:?}");
        assert!(offset.y.abs() < 1e-4 && offset.z.abs() < 1e-4);
    }

    /// Dragging back to where the grab started returns the entity exactly to
    /// its start — the property that makes deltas start-relative rather than
    /// accumulated.
    #[test]
    fn dragging_out_and_back_is_a_no_op() {
        let start = GizmoTransform {
            translation: Vec3::new(1.0, 2.0, 3.0),
            ..identity()
        };
        let grab = Ray::new(Vec3::new(1.0, 2.0, 8.0), Vec3::new(0.0, 0.0, -1.0));
        let mut drag = GizmoDrag::begin(
            GizmoMode::Move,
            GizmoAxis::X,
            start.translation,
            &WORLD,
            1.0,
            &grab,
        )
        .expect("grab resolves");

        for x in [3.0, 7.0, -4.0, 1.0] {
            let ray = Ray::new(Vec3::new(x, 2.0, 8.0), Vec3::new(0.0, 0.0, -1.0));
            let delta = drag.update(&ray).expect("grab resolves");
            if x == 1.0 {
                let now = delta.apply(start);
                assert!(
                    (now.translation - start.translation).length() < 1e-4,
                    "back at the grab point, got {:?}",
                    now.translation
                );
            }
        }
    }

    /// A quarter turn dragged on the Y ring rotates the entity a quarter turn
    /// about Y, and leaves scale alone.
    #[test]
    fn dragging_a_ring_rotates_about_it() {
        let mut drag = GizmoDrag::begin(
            GizmoMode::Rotate,
            GizmoAxis::Y,
            Vec3::ZERO,
            &WORLD,
            1.0,
            &from_above(1.0, 0.0),
        )
        .expect("the ray meets the XZ plane");

        // Same plane, a quarter turn round.
        let delta = drag
            .update(&from_above(0.0, 1.0))
            .expect("still meets the plane");

        let GizmoDelta::Rotate { angle, axis, .. } = delta else {
            panic!("Rotate must yield a rotation, got {delta:?}");
        };
        assert!(
            (angle.abs() - std::f32::consts::FRAC_PI_2).abs() < 1e-3,
            "expected a quarter turn, got {angle}"
        );
        assert_eq!(axis, Vec3::Y);
        assert_eq!(delta.apply(identity()).scale, Vec3::new(1.0, 1.0, 1.0));
    }

    /// Winding past half a turn keeps going instead of snapping back — the
    /// reason the angle is accumulated rather than measured against the grab.
    #[test]
    fn rotation_winds_past_half_a_turn() {
        let mut drag = GizmoDrag::begin(
            GizmoMode::Rotate,
            GizmoAxis::Y,
            Vec3::ZERO,
            &WORLD,
            1.0,
            &from_above(1.0, 0.0),
        )
        .expect("grab resolves");

        // Eighth-turn steps: one jump of this size would wrap, stepping does not.
        let mut angle = 0.0;
        for step in 1..=8 {
            let theta = std::f32::consts::FRAC_PI_4 * step as f32;
            let delta = drag
                .update(&from_above(theta.cos(), theta.sin()))
                .expect("grab resolves");
            let GizmoDelta::Rotate { angle: a, .. } = delta else {
                panic!("expected a rotation");
            };
            angle = a;
        }
        assert!(
            angle.abs() > std::f32::consts::PI,
            "expected more than half a turn, got {angle}"
        );
    }

    /// Pulling the X handle out to twice the gizmo's size doubles the X scale
    /// and leaves Y and Z untouched.
    #[test]
    fn dragging_a_scale_handle_scales_one_axis() {
        let mut drag = GizmoDrag::begin(
            GizmoMode::Scale,
            GizmoAxis::X,
            Vec3::ZERO,
            &WORLD,
            2.0,
            &from_front(0.0, 0.0),
        )
        .expect("grab resolves");

        let result = drag
            .update(&from_front(2.0, 0.0))
            .expect("grab resolves")
            .apply(identity());
        assert!((result.scale.x - 2.0).abs() < 1e-4, "got {:?}", result.scale);
        assert_eq!(result.scale.y, 1.0);
        assert_eq!(result.scale.z, 1.0);
    }

    /// Dragging a scale handle far backwards clamps instead of inverting the
    /// entity — a negative scale is a mirror nobody asked for.
    #[test]
    fn scale_never_goes_negative() {
        let mut drag = GizmoDrag::begin(
            GizmoMode::Scale,
            GizmoAxis::X,
            Vec3::ZERO,
            &WORLD,
            1.0,
            &from_front(0.0, 0.0),
        )
        .expect("grab resolves");

        let result = drag
            .update(&from_front(-50.0, 0.0))
            .expect("grab resolves")
            .apply(identity());
        assert!(result.scale.x >= MIN_SCALE_FACTOR, "got {:?}", result.scale);
    }

    /// The scale handles follow the object, the move handles do not: that split
    /// is the reason `gizmo_basis` takes the rotation at all.
    #[test]
    fn only_scale_uses_the_object_frame() {
        let quarter = Quaternion::from_axis_angle(Vec3::Y, std::f32::consts::FRAC_PI_2);
        assert_eq!(gizmo_basis(GizmoMode::Move, quarter), WORLD);
        assert_eq!(gizmo_basis(GizmoMode::Rotate, quarter), WORLD);

        let scale = gizmo_basis(GizmoMode::Scale, quarter);
        // A quarter turn about +Y takes +X onto -Z.
        assert!(
            (scale[0] - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-4,
            "got {:?}",
            scale[0]
        );
    }

    /// Rotating a selection turns each entity *around the pivot*, so several
    /// entities move as one body rather than each spinning in place.
    #[test]
    fn rotation_orbits_the_pivot() {
        let start = GizmoTransform {
            translation: Vec3::new(2.0, 0.0, 0.0),
            ..identity()
        };
        let delta = GizmoDelta::Rotate {
            axis: Vec3::Y,
            angle: std::f32::consts::FRAC_PI_2,
            pivot: Vec3::ZERO,
        };
        let moved = delta.apply(start).translation;
        // +X rotated a quarter turn about +Y lands on -Z.
        assert!(
            (moved - Vec3::new(0.0, 0.0, -2.0)).length() < 1e-3,
            "got {moved:?}"
        );
    }

    /// An entity sitting at the pivot only spins — the common single-selection
    /// case must not drift.
    #[test]
    fn rotation_at_the_pivot_does_not_translate() {
        let delta = GizmoDelta::Rotate {
            axis: Vec3::Y,
            angle: 1.0,
            pivot: Vec3::ZERO,
        };
        assert!(delta.apply(identity()).translation.length() < 1e-5);
    }

    /// The two perpendiculars are unit, mutually orthogonal, and orthogonal to
    /// the axis — for every axis, including the ones that would make a naive
    /// fixed seed degenerate.
    #[test]
    fn perpendiculars_stay_orthonormal_on_every_axis() {
        for axis in [Vec3::X, Vec3::Y, Vec3::Z, Vec3::new(1.0, 1.0, 1.0).normalize()] {
            let (u, v) = perpendiculars(axis);
            assert!((u.length() - 1.0).abs() < 1e-4, "u not unit for {axis:?}");
            assert!((v.length() - 1.0).abs() < 1e-4, "v not unit for {axis:?}");
            assert!(u.dot(v).abs() < 1e-4, "u·v for {axis:?}");
            assert!(u.dot(axis).abs() < 1e-4, "u·axis for {axis:?}");
        }
    }

    /// Angles fold into a half-open turn, so a wrapped increment is never a
    /// near-full-turn jump in the wrong direction.
    #[test]
    fn angles_fold_into_a_single_turn() {
        use std::f32::consts::{PI, TAU};
        assert!((wrap_signed(0.5) - 0.5).abs() < 1e-6);
        assert!((wrap_signed(TAU + 0.5) - 0.5).abs() < 1e-6);
        assert!((wrap_signed(-TAU - 0.5) + 0.5).abs() < 1e-6);
        assert!(wrap_signed(PI + 0.1) < 0.0, "just past π folds negative");
    }
}
