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

//! The three levels of the one shadows lane, and the atlas each one draws
//! into. They replace the three lanes the shadow agent held
//! (`StandardShadowsLane`, `MediumShadowsLane`, `LowResShadowsLane`) and the
//! agent's strategy mapping.

use khora_core::agent::gorna::StrategyId;
use khora_core::lane::level::Impact;

use khora_core::math::Vec3;

use super::fixtures::{
    faces, view_proj, Scene, Shadows, ATLAS_LIGHTS, CUBE_LIGHTS, LEVEL_MEMORY, TIERS,
};

/// **One shadow lane offers three levels**, cheapest first: `LowPower`,
/// `Balanced`, `HighPerformance`. Each holds the graphics memory of its two
/// atlases at four bytes a texel — 5.5, 22 and 88 MiB, the quotes the shadow
/// agent made — and only the dearest is the reference result: a cheaper one
/// fills smaller atlases, which a player can see.
#[test]
fn one_shadow_lane_offers_three_levels() {
    let shadows = Shadows::new();
    let levels = shadows.lane().levels().to_vec();

    let ids: Vec<StrategyId> = levels.iter().map(|level| level.id).collect();
    assert_eq!(
        ids,
        [
            StrategyId::LowPower,
            StrategyId::Balanced,
            StrategyId::HighPerformance
        ],
        "three levels, cheapest first"
    );
    let memory: Vec<u64> = levels.iter().map(|level| level.memory.gpu).collect();
    assert_eq!(memory, LEVEL_MEMORY, "each level holds its two atlases");
    assert!(
        levels
            .iter()
            .all(|level| level.memory.cpu == 0 && level.memory.npu == 0),
        "the atlases are on the graphics device: {levels:?}"
    );
    assert_eq!(
        levels[2].impact,
        Impact::None,
        "the dearest is the reference"
    );
    assert!(
        levels[0].impact >= levels[1].impact && levels[1].impact > Impact::None,
        "a cheaper level looks worse, never better: {levels:?}"
    );
    let mut names: Vec<&str> = levels.iter().map(|level| level.name).collect();
    assert!(
        names.iter().all(|name| !name.is_empty()),
        "named: {names:?}"
    );
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 3, "three distinct names: {names:?}");
}

/// **Each level builds its atlases at its own resolution.** Recorded at a
/// level, the lane holds exactly two depth atlases: a 2D one of
/// `ATLAS_LIGHTS` square layers, and a cube one of `CUBE_LIGHTS` cubes of six
/// faces, at the level's sides — 512/128, 1024/256, 2048/512 texels.
#[test]
fn each_level_builds_its_atlases_at_its_tier_resolution() {
    for (level, side, face) in TIERS {
        let mut shadows = Shadows::new();
        let recorded = shadows.record(level, &Scene::new().with_sun(view_proj(1.0)));
        recorded.recorded();

        let mut expected = vec![(side, ATLAS_LIGHTS), (face, CUBE_LIGHTS * 6)];
        expected.sort_unstable();
        assert_eq!(
            shadows.device.depth_textures(),
            expected,
            "level {level}: a {side}² 2D atlas and a {face}² cube atlas"
        );
    }
}

/// **Every level shadows the same lights.** A cheaper level fills smaller
/// atlases, never a different set of shadowed lights: with more suns and
/// point lights than the atlases hold, each level gives the same lights the
/// same entries — same layer, same cube, same matrices.
#[test]
fn every_level_shadows_the_same_lights() {
    let mut scene = Scene::new();
    for k in 0..ATLAS_LIGHTS + 2 {
        scene = scene.with_sun(view_proj(k as f32 + 1.0)).with_lamp(
            Vec3::new(k as f32, 2.0, 0.0),
            10.0,
            faces(k as f32 + 1.0),
        );
    }
    let lights = scene.render_world.lights.len();

    let entries: Vec<Vec<String>> = TIERS
        .iter()
        .map(|&(level, _, _)| {
            let mut shadows = Shadows::new();
            let recorded = shadows.record(level, &scene);
            let (frame, _) = recorded.recorded();
            (0..lights)
                .map(|light| format!("{:?}", frame.entry(light)))
                .collect()
        })
        .collect();

    assert_eq!(
        entries[0]
            .iter()
            .filter(|entry| entry.as_str() != "None")
            .count(),
        (ATLAS_LIGHTS + CUBE_LIGHTS) as usize,
        "the atlases are full: {:?}",
        entries[0]
    );
    assert!(
        entries.windows(2).all(|pair| pair[0] == pair[1]),
        "each level shadows the same lights the same way: {entries:#?}"
    );
}
