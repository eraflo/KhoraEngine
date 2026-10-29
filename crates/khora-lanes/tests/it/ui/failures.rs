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

//! What the UI lane does when the device refuses something it asks for: the
//! frame fails and commits nothing, what was built before the refusal is
//! kept or given back — never leaked, never destroyed twice — no text of
//! the failed frame is left queued for the next, and the next frame builds
//! what is missing and draws.

use khora_core::lane::erased::RunOutcome;

use super::fixtures::{
    assert_everything_released, nth_text, queued, GpuKind, Ledger, Scene, Ui, UI_SHADER,
};

/// How many objects a whole build of the lane asks the device for, the
/// atlas bind group included.
fn objects_of_a_build() -> usize {
    let mut ui = Ui::new();
    ui.record(&Scene::new()).recorded();
    ui.device.asked()
}

/// **A refusal halfway through the UI's build leaks nothing.** For every
/// step of the build, a lane whose device refuses that step: the frame fails
/// and commits nothing; the next frame, the device willing, builds, draws,
/// and flushes its own text only; and after a shutdown nothing the lane ever
/// made is left alive or was destroyed twice.
#[test]
fn a_refusal_halfway_through_the_ui_build_leaks_nothing() {
    let steps = objects_of_a_build();
    assert!(steps > 1, "a build makes several objects: {steps}");

    for refused in 1..=steps {
        let mut ui = Ui::on(Ledger::refusing_the(refused));
        let scene = Scene::new();

        let failed = ui.record(&scene);
        assert!(
            matches!(failed.run.outcome, RunOutcome::Failed(_)),
            "refusing object {refused} of {steps} fails the frame: {:?}",
            failed.run.outcome
        );
        assert!(failed.pass.is_none(), "a failed frame commits no pass");

        ui.device.stop_refusing();
        ui.record(&scene).recorded();
        assert!(
            !ui.device.passes().is_empty(),
            "after refusing object {refused}, the next frame draws"
        );
        let flushes = ui.text.flushes();
        assert_eq!(
            flushes.last().map(|flush| flush.texts.clone()),
            Some(vec![queued(&nth_text(0))]),
            "after refusing object {refused}, the next frame flushes its own text only: {flushes:?}"
        );
        ui.shut_down();
        assert_everything_released(&ui.device);
    }
}

/// **A refused pipeline keeps the UI's buffers for the retry.** A refused
/// pipeline fails the frame twice; once it is allowed, the lane draws with
/// it, having built its buffers no more often than one build does; its
/// shutdown releases everything.
#[test]
fn a_refused_ui_pipeline_keeps_the_buffers_for_the_retry() {
    let buffers_of_a_build = {
        let mut ui = Ui::new();
        ui.record(&Scene::new()).recorded();
        ui.device.made_of(GpuKind::Buffer).len()
    };
    let mut ui = Ui::new();
    let scene = Scene::new();
    ui.pipelines.refuse(UI_SHADER);

    ui.record(&scene).failed();
    ui.record(&scene).failed();
    ui.pipelines.stop_refusing();
    ui.record(&scene).recorded();

    let made = ui.device.made_of(GpuKind::Buffer);
    assert_eq!(
        made.len(),
        buffers_of_a_build,
        "the retries build the buffers no more often than one build: {made:?}"
    );
    let built = ui.pipelines.of(UI_SHADER);
    assert_eq!(built.len(), 1, "built once it is allowed");
    assert_eq!(ui.quad_pass(0).pipelines, [built[0].id]);
    ui.shut_down();
    assert_everything_released(&ui.device);
}
