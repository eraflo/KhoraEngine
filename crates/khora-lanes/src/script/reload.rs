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

//! Applying the modules an author just recompiled, and announcing to the
//! script flow what the loaded programs read.

use khora_core::projection::Interest;
use khora_core::script::{ComponentName, ScriptReads};
use khora_script::reload::ScriptReload;

use crate::script::ScriptRuntime;

/// Applies the modules recompiled this frame, reporting what each cost.
///
/// Reported here so an author hears that a rename dropped a field's value. A
/// program whose reads `demand` does not cover waits for a frame whose
/// projection holds them, so its first `Get` never reads `null` for a
/// component that exists; without a demand nothing waits. See the book:
/// concepts/scripting/saves-and-edits.md, "Edits while running".
pub(crate) fn apply_reloads(
    runtime: &mut ScriptRuntime,
    reloads: &[ScriptReload],
    demand: Option<&Interest<ScriptReads>>,
) {
    // A waiting version a newer reload of the same module replaced is code
    // the author has already thrown away: it never runs.
    let mut waiting = runtime.take_deferred();
    waiting.retain(|kept| !reloads.iter().any(|newer| newer.module == kept.module));
    for reload in waiting.into_iter().chain(reloads.iter().cloned()) {
        // Gone — its file deleted, moved out of the script folder, or its
        // root unmounted: nothing keeps running code whose file is not there.
        let Some(program) = reload.program.as_ref() else {
            runtime.withdraw(&reload.module);
            log::info!(
                "hot-reload: `{}` is gone, its instances stop",
                reload.module
            );
            if let Some(demand) = demand {
                demand.withdraw(&ScriptReads::incoming(&reload.module));
            }
            continue;
        };
        let reads: Vec<ComponentName> = program
            .reads
            .iter()
            .map(|read| ComponentName::new(read.as_str()))
            .collect();
        if demand.is_some_and(|demand| !demand.covers(&reads)) {
            runtime.defer(reload);
            continue;
        }
        apply(runtime, &reload);
        if let Some(demand) = demand {
            demand.withdraw(&ScriptReads::incoming(&reload.module));
        }
    }
}

fn apply(runtime: &mut ScriptRuntime, reload: &ScriptReload) {
    let Some(program) = reload.program.clone() else {
        runtime.withdraw(&reload.module);
        return;
    };
    for report in runtime.reload(&reload.module, program) {
        if report.lost_anything() {
            log::warn!(
                "hot-reload: `{}` lost {} — a renamed field keeps no value",
                report.behavior,
                report.dropped.join(", ")
            );
        } else {
            log::info!(
                "hot-reload: `{}` kept {} field(s)",
                report.behavior,
                report.kept.len()
            );
        }
    }
}

/// Announces to `demand` every component the loaded and waiting programs read,
/// for the script flow, which runs next, to project.
///
/// Recomputed every frame from the programs themselves, so a program installed
/// any other way than a reload is not missed.
pub(crate) fn publish_reads(runtime: &ScriptRuntime, demand: &Interest<ScriptReads>) {
    let reads: Vec<ComponentName> = runtime
        .programs()
        .flat_map(|program| program.reads.iter())
        .chain(
            runtime
                .deferred()
                .iter()
                .flat_map(|reload| reload.program.iter())
                .flat_map(|program| program.reads.iter()),
        )
        .map(|read| ComponentName::new(read.as_str()))
        .collect();
    demand.announce(ScriptReads::LOADED, reads);
}
