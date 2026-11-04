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

//! What a render lane quotes: real time per device — the host's recording,
//! the device's vertices, materials, lights and tiles — the memory its level
//! holds, and, for the forward lane, the impact of a frame past its light
//! caps.

use khora_core::lane::level::Impact;

use super::fixtures::{sized, Renderer, Scene, Which, EVERY_LANE, LIT_LANES};

/// The impact the forward lane quotes over `scene`, its level's when the
/// quote names none.
fn forward_impact(scene: &Scene) -> Impact {
    let renderer = Renderer::new(Which::Forward);
    let level = renderer.levels()[0].impact;
    renderer.cost_of(scene).impact.unwrap_or(level)
}

/// **The forward lane quotes `Minor` past its light caps.** It shades at most
/// 4 directional, 16 point and 8 spot lights: one more of any kind, and the
/// frame it would draw is not the reference image.
#[test]
fn the_forward_lane_quotes_minor_past_its_light_caps() {
    for (what, scene) in [
        ("5 suns", Scene::new().with_meshes(1).with_suns(5)),
        ("17 lamps", Scene::new().with_meshes(1).with_lamps(17)),
        ("9 spots", Scene::new().with_meshes(1).with_spots(9)),
    ] {
        assert_eq!(forward_impact(&scene), Impact::Minor, "{what}");
    }
}

/// **Within its caps the forward lane quotes no impact**: 4 suns, 16 lamps
/// and 8 spots at once is the reference image.
#[test]
fn within_its_caps_the_forward_lane_quotes_no_impact() {
    let scene = Scene::new()
        .with_meshes(1)
        .with_suns(4)
        .with_lamps(16)
        .with_spots(8);

    assert_eq!(forward_impact(&scene), Impact::None);
    assert_eq!(
        forward_impact(&Scene::new()),
        Impact::None,
        "no light at all"
    );
}

/// **Forward+ quotes no impact past the forward lane's caps**: it culls its
/// lights per tile, so seventeen lamps are still the reference image.
#[test]
fn forward_plus_quotes_no_impact_past_the_forward_caps() {
    let renderer = Renderer::new(Which::ForwardPlus);
    let level = renderer.levels()[0].impact;

    let cost = renderer.cost_of(&Scene::new().with_meshes(1).with_lamps(17).with_suns(5));

    assert_eq!(cost.impact.unwrap_or(level), Impact::None);
}

/// **Every lane costs more, on the host and on the device, with more
/// meshes**: each mesh is recorded on the host and its vertices and material
/// shaded on the device.
#[test]
fn every_lane_costs_more_on_each_device_with_more_meshes() {
    for which in EVERY_LANE {
        let renderer = Renderer::new(which);

        let one = renderer.cost_of(&Scene::new().with_meshes(1));
        let twenty = renderer.cost_of(&Scene::new().with_meshes(20));

        assert!(
            twenty.time.cpu > one.time.cpu,
            "{which:?}: the host records more draws ({:?} vs {:?})",
            twenty.time.cpu,
            one.time.cpu
        );
        assert!(
            twenty.time.gpu > one.time.gpu,
            "{which:?}: the device shades more ({:?} vs {:?})",
            twenty.time.gpu,
            one.time.gpu
        );
    }
}

/// **A lit lane costs the device more with more lights**: each fragment
/// shades every light it sees.
#[test]
fn a_lit_lane_costs_the_device_more_with_more_lights() {
    for which in LIT_LANES {
        let renderer = Renderer::new(which);

        let dark = renderer.cost_of(&Scene::new().with_meshes(4));
        let lit = renderer.cost_of(&Scene::new().with_meshes(4).with_lamps(8));

        assert!(
            lit.time.gpu > dark.time.gpu,
            "{which:?}: eight lamps cost the device ({:?} vs {:?})",
            lit.time.gpu,
            dark.time.gpu
        );
    }
}

/// **Forward+ costs the device more on a larger target**: it culls the
/// lights of every tile of the target.
#[test]
fn forward_plus_costs_the_device_more_on_a_larger_target() {
    let renderer = Renderer::new(Which::ForwardPlus);
    let scene = || Scene::new().with_meshes(4).with_lamps(8);

    let small = renderer.cost_of(&scene().into_target(sized(640, 480)));
    let large = renderer.cost_of(&scene().into_target(sized(3840, 2160)));

    assert!(
        large.time.gpu > small.time.gpu,
        "a 4K target has more tiles to cull ({:?} vs {:?})",
        large.time.gpu,
        small.time.gpu
    );
}

/// **A quote holds the memory its level holds**: the graphics memory GORNA
/// shares out is the level's.
#[test]
fn every_lane_quotes_the_memory_its_level_holds() {
    for which in EVERY_LANE {
        let renderer = Renderer::new(which);
        let level = renderer.levels()[0];

        let cost = renderer.cost_of(&Scene::new().with_meshes(3));

        assert_eq!(cost.memory, level.memory, "{which:?}");
    }
}

/// **A quote runs nothing.** Quoting a lane over a scene builds nothing on the
/// device and opens no encoder: the agent prices rows it does not run.
#[test]
fn a_quote_builds_nothing() {
    for which in EVERY_LANE {
        let renderer = Renderer::new(which);

        let _ = renderer.quote(&Scene::new().with_meshes(3).with_lamps(2));

        assert!(
            renderer.device.lane_made().is_empty(),
            "{which:?}: {:?}",
            renderer.device.lane_made()
        );
        assert!(renderer.device.lane_opened().is_empty(), "{which:?}");
        assert!(renderer.pipelines.lane_pipelines().is_empty(), "{which:?}");
    }
}
