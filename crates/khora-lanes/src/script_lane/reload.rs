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

//! Applying the modules an author just recompiled, and keeping the catalog of
//! what the loaded programs read.

use std::sync::Arc;

use khora_core::script::{ComponentName, ComponentProjection, ScriptCatalog};
use khora_script::reload::ScriptReload;

use crate::script_lane::ScriptRuntime;

/// Applies the modules recompiled this frame, reporting what each cost.
///
/// The reporting is the point of doing it here rather than inside the runtime:
/// a rename drops a field's value, and an author who is not told is left to
/// discover it in whatever the guard does next.
///
/// A program that reads a component this frame's view did not project waits
/// for the next frame, whose view will — its first `Get` must not read `null`
/// for a component that exists. Without a catalog, nothing projects, so
/// nothing waits.
pub(crate) fn apply_reloads(
    runtime: &mut ScriptRuntime,
    reloads: &[ScriptReload],
    projected: &ComponentProjection,
    catalog: Option<&Arc<ScriptCatalog>>,
) {
    // A waiting version a newer reload of the same module replaced is code
    // the author has already thrown away: it never runs.
    let mut waiting = runtime.take_deferred();
    waiting.retain(|kept| !reloads.iter().any(|newer| newer.module == kept.module));
    for reload in waiting.into_iter().chain(reloads.iter().cloned()) {
        let unprojected = reload
            .program
            .reads
            .iter()
            .any(|read| projected.component(read).is_none());
        if catalog.is_some() && unprojected {
            runtime.defer(reload);
            continue;
        }
        apply(runtime, &reload);
        if let Some(catalog) = catalog {
            catalog.settle(&reload.module);
        }
    }
}

fn apply(runtime: &mut ScriptRuntime, reload: &ScriptReload) {
    for report in runtime.reload(&reload.module, reload.program.clone()) {
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

/// Writes to `catalog` every component the loaded programs read — and the
/// waiting ones, so the next frame projects what they need.
///
/// Recomputed every frame from the programs themselves rather than only on a
/// reload: a handful of programs is cheap to walk, and a program installed any
/// other way is not missed.
pub(crate) fn publish_reads(runtime: &ScriptRuntime, catalog: &ScriptCatalog) {
    let reads = runtime
        .programs()
        .flat_map(|program| program.reads.iter())
        .chain(
            runtime
                .deferred()
                .iter()
                .flat_map(|reload| reload.program.reads.iter()),
        )
        .map(|read| ComponentName::new(read.as_str()))
        .collect();
    catalog.set_reads(reads);
}
