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

use khora_core::math::{Quat, Vec3};
use rapier3d::na::{Point3, Quaternion, UnitQuaternion, Vector3};
use rapier3d::prelude::Real;

pub fn to_rapier_vec(v: Vec3) -> Vector3<Real> {
    Vector3::new(v.x, v.y, v.z)
}

pub fn to_rapier_point(v: Vec3) -> Point3<Real> {
    Point3::new(v.x, v.y, v.z)
}

pub fn to_rapier_quat(q: Quat) -> UnitQuaternion<Real> {
    UnitQuaternion::from_quaternion(Quaternion::new(q.w, q.x, q.y, q.z))
}

pub fn from_rapier_vec(v: Vector3<Real>) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

pub fn from_rapier_point(p: Point3<Real>) -> Vec3 {
    Vec3::new(p.x, p.y, p.z)
}

pub fn from_rapier_quat(q: UnitQuaternion<Real>) -> Quat {
    Quat::new(q.i, q.j, q.k, q.w)
}
