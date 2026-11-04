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

//! Forward+ uploads at most as many lights as its light buffer holds, 1024;
//! a frame with more is drawn without the ones past it, and its quote says
//! so — and the frame is still drawn.

use khora_core::lane::level::Impact;

use super::fixtures::{Renderer, Scene, Which};

/// The lights Forward+'s light buffer holds.
const LIGHT_BUFFER_CAPACITY: usize = 1024;

/// **Forward+ quotes `Minor` past its light buffer, and still draws.** At
/// 1024 lamps its quote names no impact; at 1025 it names `Minor`, and the
/// frame records, drawing its mesh.
#[test]
fn forward_plus_quotes_minor_past_its_light_buffer_and_still_draws() {
    let mut renderer = Renderer::new(Which::ForwardPlus);
    let level = renderer.levels()[0].impact;

    let full = Scene::new()
        .with_meshes(1)
        .with_lamps(LIGHT_BUFFER_CAPACITY);
    assert_eq!(
        renderer.cost_of(&full).impact.unwrap_or(level),
        Impact::None,
        "a full light buffer is the reference image"
    );

    let past = Scene::new()
        .with_meshes(1)
        .with_lamps(LIGHT_BUFFER_CAPACITY + 1);
    assert_eq!(
        renderer.cost_of(&past).impact,
        Some(Impact::Minor),
        "one light past the buffer is not"
    );

    let recorded = renderer.record(&past);
    let (scene_pass, _) = recorded.recorded();
    let draws: usize = renderer
        .device
        .passes_into(scene_pass.command_buffer)
        .iter()
        .map(|pass| pass.draws.len())
        .sum();
    assert_eq!(draws, 1, "the frame past the buffer is still drawn");
}
