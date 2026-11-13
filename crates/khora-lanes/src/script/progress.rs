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

//! What one behavior's turn reads, and where its instance has got to: the
//! invocation the frame hands a turn, and the progress the turn advances and
//! hands back.

use khora_core::script::EventQueue;
use khora_script::vm::Program;

use super::runtime::{Body, Pending};

/// Everything one behavior's turn reads, other than the host it runs against.
///
/// Grouped rather than passed loose: they describe a single invocation and
/// always travel together, so a call site cannot get two of them out of order.
pub(super) struct Invocation<'a> {
    pub(super) program: &'a Program,
    pub(super) behavior: &'a str,
    pub(super) entity: khora_core::ecs::entity::EntityId,
    pub(super) events: &'a EventQueue,
    pub(super) fuel: u64,
    /// The seconds since the instance's last turn — the frame's, or more
    /// when a rate or the fuel kept it out of some: what its countdowns and
    /// waits count down, before anything runs.
    pub(super) delta: f32,
    /// What its `Update` is given: `delta`, and whatever earlier turns that
    /// stopped before their `Update` left owed to it.
    pub(super) update_delta: f32,
    /// The seconds since the previous frame: what the watchdog counts a body
    /// as running, whatever it was owed.
    pub(super) frame_delta: f32,
}

/// Where an instance has got to — what a turn advances.
///
/// The frame lends it to the turn and writes it back whatever happened, so a
/// turn that stops early cannot lose a body part-way through.
pub(super) struct Progress {
    /// Whether it has announced itself through `OnSpawn`.
    pub(super) spawned: bool,
    /// The body part-way through, if there is one.
    pub(super) pending: Option<Pending>,
    /// Whether `OnLoad` is owed or part-way.
    pub(super) loading: bool,
    /// The body a load restored, waiting behind `OnLoad`.
    pub(super) after_load: Option<Pending>,
    /// The members of bodies an edit left nothing of, each owed
    /// `OnResumeFailed`, oldest first.
    pub(super) resume_failed: Vec<String>,
    /// A body this turn found running past the watchdog's first limit: its
    /// member and where it stands.
    pub(super) slow: Option<(String, String)>,
    /// The events this turn dropped, a payload their handler could not take:
    /// each event and why.
    pub(super) dropped: Vec<(String, String)>,
    /// The fixed steps this turn is to call `FixedUpdate` for and has not,
    /// each the step it was taken at: what it hands back, owed to the next
    /// turn.
    pub(super) fixed_left: Vec<f32>,
    /// The time its `Update` is owed and has not been handed this turn —
    /// `Invocation::update_delta` until `Update` is called: what it keeps,
    /// owed to the next turn, when the turn stops before it.
    pub(super) update_left: f32,
}

/// `OnLoad` has finished: the body the load restored is the one owed next.
pub(super) fn finish_load(progress: &mut Progress) {
    progress.loading = false;
    progress.pending = progress.after_load.take();
}

/// A hook in front of a restored body has finished — or been abandoned: that
/// body is owed next.
pub(super) fn finish_hook(progress: &mut Progress, body: &Body) {
    match body {
        Body::Load => finish_load(progress),
        Body::Spawn => {
            if let Some(restored) = progress.after_load.take() {
                progress.pending = Some(restored);
            }
        }
        Body::Sequence | Body::Update | Body::Timer { .. } => {}
    }
}
