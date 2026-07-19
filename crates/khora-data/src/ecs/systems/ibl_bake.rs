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

//! One-time IBL environment bake, wired as a data-driven system.
//!
//! Runs in [`TickPhase::PreExtract`] so the baked environment is ready before
//! any lit lane renders. The [`IblBaker`] bakes only on its first call and is
//! a cheap no-op every frame after — no dirty tracking needed here.

use std::sync::Arc;

use khora_core::lane::OutputDeck;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;
use khora_core::Runtime;

use crate::ecs::{DataSystemRegistration, TickPhase, World};
use crate::IblBaker;

fn ibl_bake_system(_world: &mut World, runtime: &Runtime, _deck: &mut OutputDeck) {
    let Some(baker) = runtime.resources.get::<IblBaker>() else {
        return;
    };
    let Some(device) = runtime.backends.get::<Arc<dyn GraphicsDevice>>() else {
        return;
    };
    let Some(pipeline_system) = runtime.resources.get::<Arc<dyn PipelineSystem>>() else {
        return;
    };
    baker.ensure_baked(device.as_ref(), pipeline_system.as_ref());
}

inventory::submit! {
    DataSystemRegistration {
        name: "ibl_bake",
        phase: TickPhase::PreExtract,
        run: ibl_bake_system,
        order_hint: -10,
        runs_after: &[],
    }
}
