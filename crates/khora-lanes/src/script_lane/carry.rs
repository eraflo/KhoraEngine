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

//! One instance carried from the program its fields were laid out for into
//! another: by a hot reload of its module, or — its behavior moved to another
//! module — at its next turn.

use std::sync::Arc;

use khora_script::vm::{Program, ResumeTier};

use super::persistence;
use super::report::Resumed;
use super::resumption;
use super::runtime::{member_of, Instance};

/// Carries `instance` of `behavior` from `old_program` — what its fields are
/// laid out for — into `program`, and returns how each body part-way came
/// back below the exact tier. Nothing to carry when either program lacks the
/// behavior.
///
/// By name: inserting one field at the top shifts every slot after it, and
/// carrying values positionally would move a guard's health into its speed.
/// The state it is in, that state's data and every countdown cross by name
/// too, so an edit does not send a chasing guard back to patrol; a field the
/// edit added starts at its default — the layout's data.
pub(super) fn carry_instance(
    instance: &mut Instance,
    old_program: &Arc<Program>,
    program: &Arc<Program>,
    behavior: &str,
) -> Vec<Resumed> {
    let (Some(old), Some(layout)) = (old_program.layout(behavior), program.layout(behavior)) else {
        return Vec::new();
    };
    let fingerprint = program.fingerprint();
    let mut resumes = Vec::new();
    instance.fields =
        persistence::carried_across(old, layout, &instance.fields, instance.pending.as_ref());
    // A body part-way through is a position in the old code. It is carried
    // into the new code here, in whatever tier the edit allows, rather than
    // when it would next run, so that nothing — a save taken in between,
    // above all — goes on holding a position the program no longer has. The
    // body a load restored, waiting behind its `OnLoad`, likewise.
    for slot in [&mut instance.pending, &mut instance.after_load] {
        let Some(pending) = slot.take() else {
            continue;
        };
        if pending.fingerprint == fingerprint {
            *slot = Some(pending);
            continue;
        }
        match resumption::carry(&pending, old_program, old, program, layout) {
            Ok((carried, tier)) => {
                if tier != ResumeTier::Exact {
                    resumes.push(Resumed {
                        behavior: behavior.to_owned(),
                        member: member_of(&carried, program),
                        tier: Ok(tier),
                    });
                }
                *slot = Some(carried);
            }
            Err(abandoned) => {
                resumes.push(Resumed {
                    behavior: behavior.to_owned(),
                    member: abandoned.member.clone(),
                    tier: Err(abandoned.clone()),
                });
                // A schedule whose body it was waits its interval: the carry
                // left it rearmed.
                instance.resume_failed.push(abandoned.member);
            }
        }
    }
    // An edit is the author's answer to whatever faulted. Refusing to try
    // again would make a script unfixable without a restart.
    instance.disabled = false;
    instance.fault = None;
    instance.built_for = Some(Arc::clone(program));
    // What the World holds of it is the old program's until it is written
    // again: written this frame, whether or not it runs.
    instance.rewritten = true;
    resumes
}
