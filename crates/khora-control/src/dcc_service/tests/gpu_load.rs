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

//! How the running DCC reads the GPU's load: the frames' measured GPU time
//! over the frame time aimed for, unknown while no frame was timed on the GPU.

use super::*;

/// Sends `count` frames, the host busy 4 ms, the GPU `gpu_ms` when timed.
fn frames(tx: &crossbeam_channel::Sender<TelemetryEvent>, count: usize, gpu_ms: Option<f64>) {
    for _ in 0..count {
        tx.send(TelemetryEvent::FrameReport(frame_report(4.0, gpu_ms)))
            .expect("the DCC listens");
    }
}

/// **A GPU-bound frame raises the GPU pressure.**
///
/// Frames keeping the host busy 4 ms and the GPU 16 ms, against the 16.66 ms
/// setpoint: the GPU is loaded 0.96, over its critical 0.95. The DCC's context
/// reads that load, and a `Pressure` epoch opens.
#[test]
fn a_gpu_bound_frame_raises_the_gpu_pressure() {
    let (mut dcc, rx) = recording(DccConfig {
        min_epoch_ticks: 1,
        refresh_every_ticks: 0,
        ..Default::default()
    });
    let (agent, _ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    dcc.register_agent(agent, 1.0);
    let tx = dcc.event_sender();
    dcc.start(rx);
    assert!(wait_for(3, || opened_by(&dcc, Trigger::Initial)));

    frames(&tx, 20, Some(16.0));
    let loaded = wait_for(3, || {
        dcc.get_context()
            .hardware
            .gpu_load
            .is_some_and(|load| load > 0.95)
    });
    let pressed = wait_for(3, || opened_by(&dcc, Trigger::Pressure));
    let read = dcc.get_context().hardware.gpu_load;
    dcc.stop();

    assert!(loaded, "the GPU load is read from the frames: {read:?}");
    assert!(pressed, "a GPU over its critical load is a pressure");
}

/// **Frames not timed on the GPU leave its load unknown, not idle.**
///
/// A backend without timestamp queries reports the host's time alone: after
/// many such frames, the DCC still knows nothing of the GPU's load.
#[test]
fn frames_not_timed_on_the_gpu_leave_its_load_unknown() {
    let (mut dcc, tx) = listening();

    frames(&tx, 20, None);
    // Many ticks over the frames.
    thread::sleep(Duration::from_millis(200));
    let read = dcc.get_context().hardware.gpu_load;
    dcc.stop();

    assert_eq!(read, None, "unknown, not 0");
}

/// **A light GPU frame reads as a light load, not unknown.**
///
/// The GPU busy 4 ms of the 16.66 ms setpoint is loaded about 0.24.
#[test]
fn a_light_gpu_frame_reads_as_a_light_load() {
    let (mut dcc, tx) = listening();

    frames(&tx, 20, Some(4.0));
    let read = wait_for(3, || dcc.get_context().hardware.gpu_load.is_some());
    let load = dcc.get_context().hardware.gpu_load;
    dcc.stop();

    assert!(read, "the frames were timed on the GPU");
    let load = load.expect("a load");
    assert!((load - 4.0 / 16.66).abs() < 0.02, "{load}");
}

/// **A hardware report without a GPU load leaves the one the frames gave.**
///
/// A hardware monitor that cannot read the GPU says nothing of it: the load
/// the frames' GPU time gave stays.
#[test]
fn a_hardware_report_without_a_gpu_load_leaves_the_frames_one() {
    let (mut dcc, tx) = listening();

    frames(&tx, 20, Some(8.0));
    assert!(wait_for(3, || dcc
        .get_context()
        .hardware
        .gpu_load
        .is_some()));
    tx.send(TelemetryEvent::HardwareReport(
        khora_core::telemetry::monitoring::HardwareReport {
            cpu_load: 0.3,
            gpu_load: None,
            ..Default::default()
        },
    ))
    .expect("the DCC listens");
    let seen = wait_for(3, || {
        (dcc.get_context().hardware.cpu_load - 0.3).abs() < 1e-6
    });
    // A few more ticks.
    thread::sleep(Duration::from_millis(50));
    let load = dcc.get_context().hardware.gpu_load;
    dcc.stop();

    assert!(seen, "the hardware report was read");
    assert!(load.is_some(), "the GPU load stays known: {load:?}");
}

/// **A monitor's GPU load stands in only while no frame is timed on the GPU.**
///
/// A monitor reports the GPU loaded 0.3. Frames timed at 16 ms load it 0.96:
/// the frames are the measure, and the monitor's reading is not read. Once
/// frames stop being timed (a window that presents nothing), the frames say
/// nothing of the GPU, and the monitor's 0.3 stands.
#[test]
fn a_monitors_gpu_load_stands_in_only_while_no_frame_is_timed() {
    let (mut dcc, tx) = listening();
    let monitor =
        TelemetryEvent::HardwareReport(khora_core::telemetry::monitoring::HardwareReport {
            gpu_load: Some(0.3),
            ..Default::default()
        });

    tx.send(monitor.clone()).expect("the DCC listens");
    let monitored = wait_for(3, || dcc.get_context().hardware.gpu_load == Some(0.3));
    frames(&tx, 20, Some(16.0));
    tx.send(monitor.clone()).expect("the DCC listens");
    let timed = wait_for(3, || {
        dcc.get_context()
            .hardware
            .gpu_load
            .is_some_and(|load| load > 0.9)
    });
    thread::sleep(Duration::from_millis(50));
    let kept = dcc.get_context().hardware.gpu_load;
    frames(&tx, 20, None);
    let back = wait_for(3, || dcc.get_context().hardware.gpu_load == Some(0.3));
    dcc.stop();

    assert!(
        monitored,
        "with no frame timed, the monitor's reading is read"
    );
    assert!(timed, "timed frames are the measure");
    assert!(
        kept.is_some_and(|load| load > 0.9),
        "a monitor's reading does not replace the frames' measure: {kept:?}"
    );
    assert!(back, "frames no longer timed leave the monitor's reading");
}

/// **Untimed frames between timed ones leave the GPU load known.**
///
/// The load ages after 10 untimed frames in a row, not 10 in all: frames
/// timed at 16 ms on the GPU, each followed by one untimed, keep the load the
/// frames give.
#[test]
fn untimed_frames_between_timed_ones_leave_the_gpu_load_known() {
    let (mut dcc, tx) = listening();

    for _ in 0..30 {
        frames(&tx, 1, Some(16.0));
        frames(&tx, 1, None);
    }
    // Many ticks over the frames.
    thread::sleep(Duration::from_millis(200));
    let load = dcc.get_context().hardware.gpu_load;
    dcc.stop();

    assert!(
        load.is_some_and(|load| load > 0.9),
        "30 untimed frames, never 2 in a row, aged the load to {load:?}"
    );
}

/// **A GPU load no frame measured since presses no round.**
///
/// Frames timed at 16 ms on the GPU load it 0.96, and `Pressure` epochs open.
/// Then the window is minimised: the frames still run, 16 ms on the host, but
/// present nothing, so none is timed on the GPU. Once a hundred such frames
/// ran, the GPU's last reading says nothing of them: no `Pressure` epoch opens
/// any more.
#[test]
fn a_gpu_load_no_frame_measured_since_presses_no_round() {
    let (mut dcc, rx) = recording(DccConfig {
        min_epoch_ticks: 1,
        refresh_every_ticks: 0,
        ..Default::default()
    });
    let (agent, _ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    dcc.register_agent(agent, 1.0);
    let tx = dcc.event_sender();
    dcc.start(rx);
    assert!(wait_for(3, || opened_by(&dcc, Trigger::Initial)));
    let pressures = |dcc: &DccService| {
        (epochs(dcc).iter())
            .filter(|epoch| epoch.trigger == Trigger::Pressure)
            .count()
    };

    frames(&tx, 20, Some(16.0));
    assert!(wait_for(3, || pressures(&dcc) > 0), "the GPU load pressed");
    for _ in 0..20 {
        for _ in 0..5 {
            tx.send(TelemetryEvent::FrameReport(frame_report(16.0, None)))
                .expect("the DCC listens");
        }
        thread::sleep(Duration::from_millis(10));
    }
    let before = pressures(&dcc);
    thread::sleep(Duration::from_millis(300));
    let after = pressures(&dcc);
    let load = dcc.get_context().hardware.gpu_load;
    dcc.stop();

    assert_eq!(
        after, before,
        "no frame was timed on the GPU since, yet the GPU load {load:?} kept pressing"
    );
}
