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

//! The families' places at a frame: the staged layers swapped in at the
//! frame start, and the agents woken for the wakes their places gathered.

use super::known::{locked, MetAgent};
use super::panic_guard::run_guarded;
use khora_core::gpu::GpuSetup;
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;
use khora_core::{EngineContext, Runtime};
use std::sync::Arc;

/// Starts each family's place's frame: swaps its staged layer in, releases the kept ones
/// it cannot keep.
///
/// A layer that panics while released fails its own family's frame start, not the frame.
pub(super) fn swap_layers(met: &[MetAgent], runtime: &Runtime) {
    let mut gpu = gpu_of(runtime);
    for (_, known) in met {
        let swapped = run_guarded(known.id, || locked(&known.place).start_frame(&mut gpu));
        if !swapped {
            known.place.clear_poison();
        }
    }
}

/// Wakes every agent whose place gathered a wake, with the permit of its own contention.
///
/// Takes the agent's lock, then its place's: the one order both are ever taken in. Touches
/// no world, and writes nothing a later agent reads.
pub(super) fn wake_agents(met: &[MetAgent], runtime: &Arc<Runtime>, bus: &LaneBus) {
    for (agent, known) in met {
        let id = &known.id;
        let why = locked(&known.place).take_wakes();
        if why.is_empty() {
            continue;
        }
        // Waking runs lane code — quotes, the preparation of a new layer —
        // so a panic there fails this family's wake, not the frame: its
        // installed layer runs on it as on any frame.
        let woken = run_guarded(*id, || {
            let mut agent = agent.lock().unwrap_or_else(|e| e.into_inner());
            let permit = agent.contention();
            let mut scratch = OutputDeck::new();
            let mut context = EngineContext::for_agent(
                Arc::clone(runtime),
                bus,
                &mut scratch,
                &permit,
                Some(*id),
            );
            let mut place = locked(&known.place);
            agent.wake(&why, &mut place, &mut context);
        });
        if !woken {
            agent.clear_poison();
            known.place.clear_poison();
            // Handed back, so the next event pass wakes the agent for them
            // again: a panic once is not a silent death, nor a lost
            // allocation.
            let mut place = locked(&known.place);
            for wake in why {
                place.raise(wake);
            }
        }
    }
}

/// Returns the device and pipeline system a layer is released with; headless without a
/// device.
fn gpu_of(runtime: &Runtime) -> GpuSetup<'_> {
    let mut gpu = match runtime.look_up::<Arc<dyn GraphicsDevice>>() {
        Some(device) => GpuSetup::new(device.as_ref()),
        None => GpuSetup::headless(),
    };
    if let Some(pipelines) = runtime.look_up::<Arc<dyn PipelineSystem>>() {
        gpu = gpu.with_pipelines(pipelines.as_ref());
    }
    gpu
}
