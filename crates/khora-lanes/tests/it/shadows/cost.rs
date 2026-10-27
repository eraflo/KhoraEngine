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

//! What each level of the shadows lane quotes on a frame's inputs: the
//! graphics memory of its atlases, and a time that grows with the depth
//! passes the frame needs — one per sun, six per point light.

use std::time::Duration;

use khora_core::lane::cost::Cost;
use khora_core::math::Vec3;

use super::fixtures::{
    faces, view_proj, Scene, Shadows, ATLAS_LIGHTS, CUBE_LIGHTS, HIGH, LEVEL_MEMORY, LOW,
};

/// The time a cost takes: its busiest device's.
fn busiest(cost: &Cost) -> Duration {
    cost.time
        .cpu
        .max(cost.time.gpu)
        .max(cost.time.npu)
        .max(cost.time.other)
}

/// The costs the run quoted for every level, in level order.
fn quoted(shadows: &mut Shadows, level: usize, scene: &Scene) -> Vec<Cost> {
    let recorded = shadows.record(level, scene);
    recorded.recorded();
    assert_eq!(
        recorded.run.costs.len(),
        3,
        "a run quotes every level: {:?}",
        recorded.run.costs
    );
    recorded.run.costs
}

/// **Each level quotes the memory of its atlases**, whatever level ran: the
/// memory a level holds is its levels' entry.
#[test]
fn each_level_quotes_the_memory_of_its_atlases() {
    let mut shadows = Shadows::new();

    let costs = quoted(&mut shadows, LOW, &Scene::new().with_sun(view_proj(1.0)));

    let memory: Vec<u64> = costs.iter().map(|cost| cost.memory.gpu).collect();
    assert_eq!(memory, LEVEL_MEMORY);
}

/// **The time grows with the depth passes** the frame needs: none for a scene
/// without casters, one per sun, six for a point light — at the same level,
/// a scene needing more passes never quotes less.
#[test]
fn the_time_grows_with_the_depth_passes() {
    let mut shadows = Shadows::new();
    let at = Vec3::new(0.0, 2.0, 0.0);

    let none = busiest(&quoted(&mut shadows, HIGH, &Scene::new())[HIGH]);
    let one_sun =
        busiest(&quoted(&mut shadows, HIGH, &Scene::new().with_sun(view_proj(1.0)))[HIGH]);
    let lamp = busiest(
        &quoted(
            &mut shadows,
            HIGH,
            &Scene::new().with_lamp(at, 10.0, faces(1.0)),
        )[HIGH],
    );

    assert!(one_sun > none, "a sun costs a pass: {none:?} → {one_sun:?}");
    assert!(
        lamp > one_sun,
        "a point light is six passes: {one_sun:?} → {lamp:?}"
    );
}

/// On one frame's inputs, a dearer level never quotes less time: it fills
/// larger atlases with the same passes.
#[test]
fn a_dearer_level_never_quotes_less_time() {
    let mut shadows = Shadows::new();
    let scene =
        Scene::new()
            .with_sun(view_proj(1.0))
            .with_lamp(Vec3::new(0.0, 2.0, 0.0), 10.0, faces(1.0));

    let times: Vec<Duration> = quoted(&mut shadows, LOW, &scene)
        .iter()
        .map(busiest)
        .collect();

    assert!(
        times.windows(2).all(|pair| pair[0] <= pair[1]),
        "cheapest first: {times:?}"
    );
    assert!(
        times[0] > Duration::ZERO,
        "a frame with casters is never free: {times:?}"
    );
}

/// **Casters beyond a level's capacity cost nothing more**: the quote counts
/// the passes the lane records, not the lights the frame holds. Two suns and
/// two point lights past both atlases quote what the full atlases quote, at
/// every level, and the lane records exactly the full atlases' passes.
#[test]
fn casters_beyond_the_atlases_cost_nothing_more() {
    let scene_of = |suns: u32, lamps: u32| {
        let mut scene = Scene::new();
        for k in 0..suns {
            scene = scene.with_sun(view_proj(k as f32 + 1.0));
        }
        for k in 0..lamps {
            scene = scene.with_lamp(Vec3::new(k as f32, 2.0, 0.0), 10.0, faces(k as f32 + 1.0));
        }
        scene
    };
    let full = scene_of(ATLAS_LIGHTS, CUBE_LIGHTS);
    let beyond = scene_of(ATLAS_LIGHTS + 2, CUBE_LIGHTS + 2);

    let at_capacity = quoted(&mut Shadows::new(), LOW, &full);
    let mut shadows = Shadows::new();
    let past_capacity = quoted(&mut shadows, LOW, &beyond);

    assert_eq!(
        shadows.device.passes().len(),
        (ATLAS_LIGHTS + CUBE_LIGHTS * 6) as usize,
        "the lane records the full atlases' passes, no more"
    );
    let times = |costs: &[Cost]| costs.iter().map(|cost| cost.time).collect::<Vec<_>>();
    assert_eq!(
        times(&past_capacity),
        times(&at_capacity),
        "the lights past the atlases add nothing to any level's quote"
    );
}
