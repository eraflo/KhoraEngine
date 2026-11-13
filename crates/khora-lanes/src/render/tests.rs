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

use super::camera_distance_sq;
use khora_core::math::{Mat4, Vec3};

fn at(x: f32, y: f32, z: f32) -> Mat4 {
    Mat4::from_translation(Vec3::new(x, y, z))
}

#[test]
fn distance_uses_the_model_translation() {
    let camera = Vec3::new(0.0, 0.0, 0.0);
    assert_eq!(camera_distance_sq(&at(3.0, 0.0, 4.0), camera), 25.0);
    // Camera offset is accounted for, not just the model position.
    assert_eq!(
        camera_distance_sq(&at(3.0, 0.0, 4.0), Vec3::new(3.0, 0.0, 0.0)),
        16.0
    );
}

#[test]
fn sorting_by_descending_distance_is_back_to_front() {
    let camera = Vec3::new(0.0, 0.0, 0.0);
    let mut draws = [
        ("near", camera_distance_sq(&at(0.0, 0.0, 1.0), camera)),
        ("far", camera_distance_sq(&at(0.0, 0.0, 9.0), camera)),
        ("mid", camera_distance_sq(&at(0.0, 0.0, 4.0), camera)),
    ];
    // The lit lanes' ordering: farthest first, so nearer transparent
    // surfaces composite over what is behind them.
    draws.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let order: Vec<&str> = draws.iter().map(|(name, _)| *name).collect();
    assert_eq!(order, ["far", "mid", "near"]);
}
