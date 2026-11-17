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

//! Taking in, every frame the scripting family is on and before the script
//! flow projects, what the engine handed the scripting world: the recompiled
//! modules and the engine's events.

use std::sync::{Arc, Mutex};

use khora_core::event::{BetweenSessions, Channel, Mailbox, Reader};
use khora_core::projection::Interest;
use khora_core::script::{ScriptEvent, ScriptReads, SCRIPT};
use khora_core::sync::LockExt;
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, SystemContext, TickPhase};
use khora_script::reload::ScriptReload;

use super::reload::{apply_reloads, publish_reads};
use super::runtime::ScriptRuntime;

/// Takes in what the engine handed `scripts` since its last frame.
///
/// Installs each reload whose program's reads `demand` covers; the others wait,
/// a newer reload of a module replacing a waiting one, and a reload with no
/// program withdraws its module. Announces what the installed and waiting
/// programs read as [`ScriptReads::LOADED`], and withdraws the
/// [`ScriptReads::incoming`] entry of each module installed or gone. Without a
/// demand nothing waits. The drained `events` join the runtime's mail.
///
/// `resumed` (`SystemContext::resumed`) clears the runtime's own mail first;
/// the channel's is kept, the frame boundary having emptied it while the
/// family was off.
pub fn take_in(
    scripts: &mut ScriptRuntime,
    reloads: Vec<ScriptReload>,
    events: Option<&Channel<ScriptEvent>>,
    demand: Option<&Interest<ScriptReads>>,
    resumed: bool,
) {
    let drained = events.map(Channel::drain).unwrap_or_default();

    // Applied before anything runs, so a frame never executes the version
    // the author has just replaced.
    apply_reloads(scripts, &reloads, demand);
    if let Some(demand) = demand {
        publish_reads(scripts, demand);
    }

    // A session starting over starts from its scene: what scripts raised in
    // the last one belongs to a world that is gone, and could reach an entity
    // whose id was given again.
    if resumed {
        scripts.take_pending();
    }

    // One queue, two producers: what scripts raised last frame, then what
    // the engine raised through its channel. The lane delivers it.
    let mut mail = scripts.take_pending();
    for event in drained {
        mail.push(event);
    }
    scripts.set_pending(mail);
}

/// Runs the intake over the engine's scripting world, when there is one.
///
/// Without one, the reserve and the event channel keep what they hold. While
/// the family is off this system does not run: the reloads the `loading`
/// family staged wait in the reserve for the first frame back.
fn script_intake(runtime: &Runtime, ctx: SystemContext) {
    let Some(scripts) = runtime.look_up::<Arc<Mutex<ScriptRuntime>>>() else {
        return;
    };
    let mut scripts = scripts.lock_recovered();
    take_in(
        &mut scripts,
        ctx.staged::<ScriptReload>(runtime),
        runtime.look_up::<Channel<ScriptEvent>>(),
        runtime
            .look_up::<Arc<Interest<ScriptReads>>>()
            .map(Arc::as_ref),
        ctx.resumed,
    );
}

// The scripting family's mailbox. An engine event is raised for the world as
// it is now: one raised while the scripts were off is not delivered to the
// next session. A recompiled module is no mail: it waits in the reserve, the
// author's latest version, for the scripts to come back on.
khora_script::inventory::submit! {
    Mailbox::of::<ScriptEvent>(SCRIPT, Reader::Take, BetweenSessions::Drop)
}

khora_script::inventory::submit! {
    DataSystemRegistration {
        name: "script_intake",
        phase: TickPhase::PreExtract,
        // After the declarations were registered, at the frame boundary;
        // before the script flow projects, which runs after this phase.
        order_hint: -7,
        run: |_, runtime, _, ctx| script_intake(runtime, ctx),
        runs_after: &[],
        observes: &[],
        role: khora_data::ecs::Role::Serves(khora_core::script::SCRIPT),
    }
}
