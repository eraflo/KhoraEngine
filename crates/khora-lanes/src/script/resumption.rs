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

//! Taking a suspended body back into the script as it is now, from a save or
//! across a hot reload, in the same tiers. See the book:
//! concepts/scripting/saves-and-edits.md, "A body part-way through, in tiers".

use khora_core::script::{FrozenMachine, PendingBody, ScriptSnapshot};
use khora_script::vm::{Abandoned, BehaviorLayout, Program, ResumeTier};

use super::persistence::{frozen_body, thawed_body};
use super::{Body, Pending};

/// Reads a suspended sequence back into the script as it is now.
///
/// Returns `None` when the save holds no sequence; otherwise the sequence in
/// the best tier the edit since allows ([`khora_script::vm::resume()`]), or
/// abandoned, the caller then owing the behavior its `OnResumeFailed`. Every
/// machine is checked against `program` here, before anything runs it: a save
/// is input nobody sized.
pub fn resume(
    saved: &ScriptSnapshot,
    program: &Program,
    layout: &BehaviorLayout,
) -> Option<Result<(Pending, ResumeTier), Abandoned>> {
    let sequence = saved.pending.as_ref()?;
    let frozen = &sequence.machine;
    let body = thawed_body(&frozen.body, program, layout);
    Some(taken_back(
        frozen,
        body,
        sequence.fingerprint,
        (sequence.remaining, sequence.running),
        program,
        &layout.name,
    ))
}

/// Carries a body part-way through from the program it stopped in to the one a
/// hot reload replaces it with.
///
/// The same tiers as a load, through the same written-down form: the machine
/// is frozen against `old`, every position named, and resumed into `new`.
pub fn carry(
    pending: &Pending,
    old: &Program,
    old_layout: &BehaviorLayout,
    new: &Program,
    new_layout: &BehaviorLayout,
) -> Result<(Pending, ResumeTier), Abandoned> {
    let body = match &pending.body {
        // Never written down, and owed by a load whatever the code.
        Body::Load => Some(Body::Load),
        other => {
            frozen_body(other, old, old_layout).and_then(|body| thawed_body(&body, new, new_layout))
        }
    };
    let Some(frozen) = pending.machine.freeze(old, PendingBody::Sequence) else {
        log::warn!(
            "script `{}`: a sequence was abandoned — it does not belong to the code it ran in",
            new_layout.name
        );
        return Err(Abandoned {
            member: String::new(),
        });
    };
    taken_back(
        &frozen,
        body,
        pending.fingerprint,
        (pending.remaining, pending.running),
        new,
        &new_layout.name,
    )
}

/// Takes `frozen` back as a body of `program`, owing `body`; a `None` body (the
/// schedule it finishes is gone) abandons it.
fn taken_back(
    frozen: &FrozenMachine,
    body: Option<Body>,
    fingerprint: u64,
    (remaining, running): (f32, f32),
    program: &Program,
    behavior: &str,
) -> Result<(Pending, ResumeTier), Abandoned> {
    let resumed = match body {
        Some(body) => khora_script::vm::resume(frozen, fingerprint, program)
            .map(|(machine, tier)| (machine, tier, body)),
        None => Err(Abandoned {
            member: member_path(frozen),
        }),
    };
    match resumed {
        Ok((machine, tier, body)) => {
            match tier {
                ResumeTier::Exact | ResumeTier::Unchanged => {}
                ResumeTier::Rebuilt => log::info!(
                    "script `{behavior}`: `{}` was edited while part-way through; it carries \
                     on where it stopped, in the new code",
                    member_path(frozen)
                ),
                ResumeTier::Restarted => log::info!(
                    "script `{behavior}`: `{}` was edited while part-way through, where it \
                     stopped is gone; it runs again from its start",
                    member_path(frozen)
                ),
            }
            Ok((
                Pending {
                    machine,
                    // A body run again from its entry has not reached its
                    // wait yet: it starts on the next turn.
                    remaining: if tier == ResumeTier::Restarted {
                        0.0
                    } else {
                        remaining
                    },
                    body,
                    fingerprint: program.fingerprint(),
                    // Run again from its entry or not, it is the body that has
                    // run that long without finishing.
                    running,
                },
                tier,
            ))
        }
        Err(abandoned) => {
            log::warn!(
                "script `{behavior}`: `{}` was abandoned — the script was edited while it was \
                 part-way through, and nothing of it can be taken back",
                abandoned.member
            );
            Err(abandoned)
        }
    }
}

/// Returns the member a frozen body belongs to: its outermost function, without
/// the behavior.
fn member_path(frozen: &FrozenMachine) -> String {
    frozen
        .frames
        .first()
        .map(|frame| {
            frame
                .function
                .split_once('.')
                .map_or(frame.function.as_str(), |(_, member)| member)
                .to_owned()
        })
        .unwrap_or_default()
}
