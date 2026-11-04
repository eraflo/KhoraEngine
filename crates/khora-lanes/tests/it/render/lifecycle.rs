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

//! A render lane gives back, at shutdown, exactly what it built: every
//! buffer, texture, view, sampler and bind group it made — its rings, its
//! neutral bindings, its cached group-3 bind groups, Forward+'s light lists
//! — once each, and nothing it does not own: not the layouts and pipelines
//! of the `PipelineSystem`, not the objects an IBL bake made, not the
//! shadows' atlases or the scene's materials.

use std::collections::HashSet;

use super::fixtures::{
    sized, GpuKind, Renderer, Scene, Surface, EVERY_LANE, OWNED_KINDS, PIPELINE_SYSTEM_IDS,
};

/// Draws a few frames with `renderer` — without shadows or IBL, then with
/// both, on two targets — so it builds all it ever builds.
fn draw_everything(renderer: &mut Renderer) {
    let bare = Scene::new()
        .with_mesh(Surface::OPAQUE, 2.0)
        .with_mesh(Surface::BLENDED, 4.0)
        .with_lamps(2);
    renderer.record(&bare).recorded();
    let lit = Scene::with_baker(renderer.bake())
        .with_mesh(Surface::OPAQUE, 2.0)
        .with_mesh(Surface::BLENDED, 4.0)
        .with_lamps(2)
        .with_shadow_frame();
    renderer.record(&lit).recorded();
    let lit = lit.into_target(sized(1920, 1080));
    renderer.record(&lit).recorded();
}

/// **Shutdown gives back exactly what each lane built.** After frames that
/// built its rings, its neutral bindings and its bind groups, a lane shut
/// down has destroyed every object it made, once, and nothing else.
#[test]
fn shutdown_gives_back_exactly_what_each_lane_built() {
    for which in EVERY_LANE {
        let mut renderer = Renderer::new(which);
        draw_everything(&mut renderer);
        let owned: Vec<(GpuKind, usize, String)> = renderer
            .device
            .lane_made()
            .into_iter()
            .filter(|made| OWNED_KINDS.contains(&made.kind))
            .map(|made| (made.kind, made.id, made.label))
            .collect();
        assert!(
            owned.iter().any(|(kind, _, _)| *kind == GpuKind::Buffer),
            "{which:?} builds no buffer, so its shutdown proves nothing"
        );

        renderer.shut_down();

        let destroyed = renderer.device.destroyed();
        let leaked: Vec<String> = owned
            .iter()
            .filter(|(kind, id, _)| !destroyed.contains(&(*kind, *id)))
            .map(|(kind, id, label)| format!("{kind:?} #{id} {label:?}"))
            .collect();
        assert!(
            leaked.is_empty(),
            "{which:?}: shutdown leaves {} of the {} objects it made alive:\n  {}",
            leaked.len(),
            owned.len(),
            leaked.join("\n  ")
        );

        let made = renderer.device.lane_made();
        let foreign: Vec<String> = destroyed
            .iter()
            .filter(|(kind, id)| !made.iter().any(|m| m.kind == *kind && m.id == *id))
            .map(|(kind, id)| {
                let owner = if *id >= PIPELINE_SYSTEM_IDS {
                    "the PipelineSystem's or the scene's"
                } else {
                    "the bake's"
                };
                format!("{kind:?} #{id} ({owner})")
            })
            .collect();
        assert!(
            foreign.is_empty(),
            "{which:?}: destroys what it does not own:\n  {}",
            foreign.join("\n  ")
        );

        let mut seen = HashSet::new();
        let twice: Vec<String> = destroyed
            .iter()
            .filter(|entry| !seen.insert(**entry))
            .map(|(kind, id)| format!("{kind:?} #{id}"))
            .collect();
        assert!(
            twice.is_empty(),
            "{which:?}: destroys the same object twice: {twice:?}"
        );
    }
}

/// **A lane never prepared gives back nothing**: its shutdown is a no-op on
/// the device, not a failure.
#[test]
fn shutting_down_a_lane_that_never_ran_destroys_nothing() {
    for which in EVERY_LANE {
        let mut renderer = Renderer::new(which);

        renderer.shut_down();

        assert!(
            renderer.device.destroyed().is_empty(),
            "{which:?}: {:?}",
            renderer.device.destroyed()
        );
    }
}
