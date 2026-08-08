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

//! Applying the modules an author just recompiled.

use crate::script_lane::ScriptRuntime;
use khora_script::reload::ScriptReload;

/// Applies the modules recompiled this frame, reporting what each cost.
///
/// The reporting is the point of doing it here rather than inside the runtime:
/// a rename drops a field's value, and an author who is not told is left to
/// discover it in whatever the guard does next.
pub(crate) fn apply_reloads(runtime: &mut ScriptRuntime, reloads: &[ScriptReload]) {
    for reload in reloads {
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
}
