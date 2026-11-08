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

//! The script compiles done off the frame, landed by ticket at its boundary.
//!
//! The `loading` family stages a `CompileDone` per root compiled, beside its
//! reloads and declarations; `script_compile_intake` takes them in after
//! `script_components` registered the declarations, and files the outcome of
//! each ticket they carry in the runtime's `CompileHub`.

use std::sync::Arc;

use khora_core::lane::{OutputDeck, Reserve};
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, SystemContext, TickPhase, World};
use khora_script::toolchain::CompileDone;

use crate::script::compile_hub::CompileHub;

/// Takes in every `CompileDone` the reserve holds, and files the outcome of
/// each of its tickets in the runtime's `CompileHub`. Nothing without a hub
/// or a reserve. A compile whose root was unmounted since lands nothing: the
/// unmount withdrew its tickets.
pub fn take_in_compiles(runtime: &Runtime) {
    let (Some(hub), Some(reserve)) = (
        runtime.resources.get::<Arc<CompileHub>>(),
        runtime.look_up::<Reserve>(),
    ) else {
        return;
    };
    for done in reserve.take::<CompileDone>() {
        if let Err(why) = &done.outcome {
            log::warn!("scripts of {}: {why}", done.root.display());
        }
        for ticket in done.tickets {
            hub.land(ticket, done.outcome.clone());
        }
    }
}

fn script_compile_intake_system(
    _world: &mut World,
    runtime: &Runtime,
    _deck: &mut OutputDeck,
    _ctx: SystemContext,
) {
    take_in_compiles(runtime);
}

// At the frame boundary, whatever families are on: an editor editing a scene
// mounts its project's scripts with scripting off.
inventory::submit! {
    DataSystemRegistration {
        name: "script_compile_intake",
        phase: TickPhase::Boundary,
        run: script_compile_intake_system,
        order_hint: 0,
        // Landed once what the compile declares is registered.
        runs_after: &["script_components"],
        observes: &[],
        role: khora_data::ecs::Role::Upkeep,
    }
}
