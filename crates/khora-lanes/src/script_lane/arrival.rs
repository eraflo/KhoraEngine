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

//! What an instance is set from the first frame its entity appears: fresh,
//! or restored from a save.

use khora_core::script::InstanceLifecycle;
use khora_script::vm::{Abandoned, ResumeTier};

use super::report::Resumed;
use super::runtime::{Instance, Pending};

/// What an entity brings the first frame it appears, read against its
/// program.
pub(super) struct Arriving {
    /// Its fields: its layout's defaults with what it brought on top.
    pub(super) store: khora_script::arena::PersistentStore,
    /// The body a save caught part-way, taken back into the script as it is
    /// now — or abandoned.
    pub(super) resumed: Option<Result<(Pending, ResumeTier), Abandoned>>,
    /// Whether it is restored from a save — observed before — rather than
    /// starting fresh: a scene entity at Play, after Stop, a runtime spawn.
    pub(super) restored: bool,
    /// What the save recorded of its life.
    pub(super) lifecycle: InstanceLifecycle,
}

/// Sets an instance from what its entity brings — wholly: nothing of an
/// earlier instance under the same entity and behavior survives an arrival.
///
/// Fresh, it has not spawned: `OnSpawn` runs once. Restored from a save, it
/// keeps whether it spawned, stays disabled by a fault recorded under this
/// very code — a fault under other code is cleared, the fix having shipped —
/// and owes `OnLoad`, with the body the save caught part-way waiting behind it
/// — or, when an edit left nothing of that body, owes `OnResumeFailed`.
///
/// Returns how that body came back, when it came back below the exact tier.
pub(super) fn arrive(
    state: &mut Instance,
    arriving: Arriving,
    behavior: &str,
    program: &khora_script::vm::Program,
) -> Option<Resumed> {
    let fingerprint = program.fingerprint();
    *state = Instance {
        fields: arriving.store,
        ..Instance::default()
    };
    if !arriving.restored {
        return None;
    }
    state.spawned = arriving.lifecycle.spawned;
    // Owed before the save, so heard before anything this load adds.
    state.resume_failed = arriving.lifecycle.resume_failed.clone();
    match arriving.lifecycle.fault {
        Some(fault) if fault.fingerprint == fingerprint => {
            state.disabled = true;
            state.fault = Some(fault);
        }
        Some(fault) => log::info!(
            "script `{behavior}` faulted before the save ({}); its code has changed since, so it \
             runs again",
            fault.reason
        ),
        None => {}
    }
    state.loading = true;
    match arriving.resumed? {
        Ok((pending, tier)) => {
            let member = super::runtime::member_of(&pending, program);
            state.after_load = Some(pending);
            (tier != ResumeTier::Exact).then(|| Resumed {
                behavior: behavior.to_owned(),
                member,
                tier: Ok(tier),
            })
        }
        Err(abandoned) => {
            state.resume_failed.push(abandoned.member.clone());
            Some(Resumed {
                behavior: behavior.to_owned(),
                member: abandoned.member.clone(),
                tier: Err(abandoned),
            })
        }
    }
}
