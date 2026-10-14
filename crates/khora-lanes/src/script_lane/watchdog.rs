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

//! How long a body may run without finishing.
//!
//! The VM bounds each frame; the watchdog bounds the *body*: one that keeps
//! running, frame after frame, never reaching its end nor an `await` — a loop
//! with no way out, cut by the fuel every frame. What it counts is game time
//! the body *ran*: a frame it is advanced, spends and is cut again adds that
//! frame's `dt`; a frame it waits on an `await`, or is not run at all, adds
//! nothing, and an `await` starts the count again. Past [`WATCHDOG_WARN`] it
//! is reported, once; past [`WATCHDOG_FAULT`] it faults like any body that
//! faults — the instance stops until an edit to its module.

use khora_core::script::RecordedFault;

use super::report::ScriptRunReport;
use super::runtime::ScriptRuntime;

/// Game seconds a body may run without finishing before it is reported.
pub const WATCHDOG_WARN: f32 = 10.0;

/// Game seconds a body may run without finishing before it faults.
pub const WATCHDOG_FAULT: f32 = 60.0;

/// What the watchdog makes of a body that ran `delta` more seconds this frame
/// and is still part-way.
#[derive(Debug, PartialEq)]
pub(super) enum Watch {
    /// Within its limits.
    Quiet,
    /// It has just run past [`WATCHDOG_WARN`].
    Slow,
    /// It has run past [`WATCHDOG_FAULT`]: the reason it faults.
    Fault(String),
}

/// A frame `held`'s behavior sat out repaying fuel its pending body overdrew:
/// run time of that body, `delta` more seconds of it — counted now, so the
/// body is reported and stopped when its count says, not when the debt is
/// paid. A body the watchdog stops is released as one whose code faulted:
/// its timer waits its interval, the body a hook stood in front of is next.
pub(super) fn sit_out(
    held: &mut super::runtime::Instance,
    program: &khora_script::vm::Program,
    behavior: &str,
    delta: f32,
) -> Option<SatOut> {
    let pending = held
        .pending
        .as_mut()
        .filter(|pending| pending.remaining <= 0.0)?;
    let machine = &pending.machine;
    let site = || machine.standing(program);
    match watch(&mut pending.running, delta, site) {
        Watch::Quiet => None,
        Watch::Slow => Some(SatOut::Slow {
            member: super::runtime::member_of(pending, program),
            site: pending.machine.standing(program),
        }),
        Watch::Fault(reason) => {
            let member = super::runtime::member_of(pending, program);
            let pending = held.pending.take()?;
            match pending.body {
                super::runtime::Body::Timer { index, rearm } => {
                    if let Some(layout) = program.layout(behavior) {
                        held.fields.set(
                            layout.timer_slot(index),
                            khora_script::arena::Persisted::Scalar(rearm),
                        );
                    }
                }
                super::runtime::Body::Load => {
                    held.loading = false;
                    held.pending = held.after_load.take();
                }
                super::runtime::Body::Spawn => held.pending = held.after_load.take(),
                super::runtime::Body::Sequence | super::runtime::Body::Update => {}
            }
            Some(SatOut::Fault(format!("`{member}` {reason}")))
        }
    }
}

/// What a frame sat out made of a body.
pub(super) enum SatOut {
    /// It has just run past [`WATCHDOG_WARN`]: its member, where it stands.
    Slow {
        /// Its member path.
        member: String,
        /// Where it stands.
        site: String,
    },
    /// It has run past [`WATCHDOG_FAULT`]: why it faults.
    Fault(String),
}

/// Adds `delta` to a pending body's `running` count and says what that makes
/// it; `site` names where the body stands. The report is made as the count
/// crosses the limit, so it is made once per run of the body — and a body
/// loaded or carried past it is not reported again.
pub(super) fn watch(running: &mut f32, delta: f32, site: impl FnOnce() -> String) -> Watch {
    let before = *running;
    *running += delta.max(0.0);
    if *running >= WATCHDOG_FAULT {
        return Watch::Fault(format!(
            "ran {:.0} s of game time without finishing — the watchdog stops a body at {WATCHDOG_FAULT:.0} s; it stood at {}",
            *running,
            site()
        ));
    }
    if before < WATCHDOG_WARN && *running >= WATCHDOG_WARN {
        return Watch::Slow;
    }
    Watch::Quiet
}

/// A frame `instance` sat out repaying the fuel its pending body overdrew,
/// counted by the watchdog as it passes — reported, or the instance stopped,
/// this frame — and recorded, so a save in between keeps the count.
pub(super) fn sat_out(
    runtime: &mut ScriptRuntime,
    instance: &khora_data::flow::ScriptInstance,
    program: &khora_data::flow::ScriptProgram,
    delta: f32,
    report: &mut ScriptRunReport,
) {
    let Some(compiled) = runtime.program(&program.module).cloned() else {
        return;
    };
    let held = runtime.instance(instance.entity, &program.behavior);
    if held.pending.is_none() {
        return;
    }
    held.rewritten = true;
    match super::watchdog::sit_out(held, &compiled, &program.behavior, delta) {
        None => {}
        Some(super::watchdog::SatOut::Slow { member, site }) => {
            log::warn!(
                "script `{}` on {:?}: `{member}` has run {} s of game time without finishing — at {site}; the watchdog stops it at {} s",
                program.behavior,
                instance.entity,
                super::watchdog::WATCHDOG_WARN,
                super::watchdog::WATCHDOG_FAULT
            );
            report.slow.push(super::report::Slow {
                entity: instance.entity,
                behavior: program.behavior.clone(),
                member,
                site,
            });
        }
        Some(super::watchdog::SatOut::Fault(reason)) => {
            log::error!(
                "script `{}` on entity {}v{} faulted and was disabled: {reason}",
                program.behavior,
                instance.entity.index,
                instance.entity.generation
            );
            held.disabled = true;
            held.fault = Some(RecordedFault {
                fingerprint: compiled.fingerprint(),
                reason,
            });
            report.faulted += 1;
        }
    }
}
