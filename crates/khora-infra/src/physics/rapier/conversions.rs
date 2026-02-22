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
use rapier3d::math::{Rotation, Vector};

pub fn to_rapier_vec(v: Vec3) -> Vector {
    Vector::new(v.x, v.y, v.z)
}

pub fn to_rapier_quat(q: Quat) -> Rotation {
    Rotation::from_xyzw(q.x, q.y, q.z, q.w)
}

pub fn from_rapier_vec(v: Vector) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

pub fn from_rapier_quat(q: Rotation) -> Quat {
    Quat::new(q.w, q.x, q.y, q.z)
}
