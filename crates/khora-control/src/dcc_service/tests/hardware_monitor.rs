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

//! The hardware monitor the DCC reads on its own thread: what it sets in the
//! context, when it refreshes it, and what it leaves to the telemetry's
//! hardware reports.

use super::*;
use khora_core::platform::{BatteryLevel, ThermalStatus};
use khora_core::telemetry::monitoring::HardwareReport;
use std::sync::atomic::AtomicUsize;

/// What a hardware monitor reads: heat, battery, the CPU's load.
type Reading = (ThermalStatus, BatteryLevel, f32);

/// The context's hardware before anything was read.
const UNREAD: Reading = (ThermalStatus::Cool, BatteryLevel::Mains, 0.0);

/// A hardware monitor whose readings move with its refreshes: after `n`
/// refreshes it reads `readings[n]`, and the last reading past the end. So a
/// reading is only seen if the DCC refreshed the monitor before reading it.
struct FakeHardware {
    readings: Vec<Reading>,
    refreshes: AtomicUsize,
}

impl FakeHardware {
    fn new(readings: Vec<Reading>) -> Arc<Self> {
        assert!(!readings.is_empty(), "a fake monitor reads something");
        Arc::new(Self {
            readings,
            refreshes: AtomicUsize::new(0),
        })
    }

    /// How many times the monitor was refreshed so far.
    fn refreshes(&self) -> usize {
        self.refreshes.load(Ordering::SeqCst)
    }

    fn reading(&self) -> Reading {
        let n = self.refreshes().min(self.readings.len() - 1);
        self.readings[n]
    }
}

impl HardwareMonitor for FakeHardware {
    fn thermal_status(&self) -> ThermalStatus {
        self.reading().0
    }
    fn battery_level(&self) -> BatteryLevel {
        self.reading().1
    }
    fn cpu_load(&self) -> f32 {
        self.reading().2
    }
    fn refresh(&self) {
        self.refreshes.fetch_add(1, Ordering::SeqCst);
    }
}

/// A DCC ticking at 100 Hz, reading `monitor`, started.
fn reading(monitor: &Arc<FakeHardware>) -> DccService {
    let (mut dcc, rx) = DccService::new(DccConfig {
        tick_rate: 100,
        ..Default::default()
    });
    dcc.set_hardware_monitor(Arc::clone(monitor) as Arc<dyn HardwareMonitor>);
    dcc.start(rx);
    dcc
}

/// The context's hardware as a [`Reading`], and its GPU load.
fn hardware(dcc: &DccService) -> (Reading, f32) {
    let hardware = dcc.get_context().hardware;
    (
        (hardware.thermal, hardware.battery, hardware.cpu_load),
        hardware.gpu_load,
    )
}

/// Whether two readings are the same, the loads to the float.
fn same(a: Reading, b: Reading) -> bool {
    a.0 == b.0 && a.1 == b.1 && (a.2 - b.2).abs() < 1e-6
}

/// A hardware report of `reading` and a GPU load of `gpu`, as the telemetry
/// sends it.
fn report(reading: Reading, gpu: f32) -> TelemetryEvent {
    TelemetryEvent::HardwareReport(HardwareReport {
        thermal: reading.0,
        battery: reading.1,
        cpu_load: reading.2,
        gpu_load: Some(gpu),
    })
}

/// **The DCC hands back the hardware monitor it was given.** Without one it
/// has none; once given one, it is that very monitor.
#[test]
fn the_dcc_hands_back_the_hardware_monitor_it_was_given() {
    let (dcc, _rx) = DccService::new(DccConfig::default());
    assert!(dcc.hardware_monitor().is_none(), "no monitor was given");

    let monitor: Arc<dyn HardwareMonitor> = FakeHardware::new(vec![UNREAD]);
    dcc.set_hardware_monitor(Arc::clone(&monitor));
    let held = dcc
        .hardware_monitor()
        .expect("the DCC holds the monitor it was given");
    assert_eq!(
        Arc::as_ptr(&held) as *const (),
        Arc::as_ptr(&monitor) as *const (),
        "the monitor handed back is the one given"
    );
}

/// **The hardware monitor sets the context once the DCC starts.** A monitor
/// that reads `Warm`, a `Low` battery and a 0.42 CPU load once refreshed:
/// the running DCC refreshes it, reads it, and the context says so.
#[test]
fn the_hardware_monitor_sets_the_context_once_the_dcc_starts() {
    let warm = (ThermalStatus::Warm, BatteryLevel::Low, 0.42);
    let monitor = FakeHardware::new(vec![UNREAD, warm]);
    let dcc = reading(&monitor);

    let set = wait_for(5, || same(hardware(&dcc).0, warm));
    assert!(
        set,
        "the context reads the monitor: {:?} after {} refreshes",
        hardware(&dcc).0,
        monitor.refreshes()
    );
}

/// **The DCC refreshes the hardware monitor.** Its readings are only fresh
/// once refreshed: the running DCC calls `refresh` on it.
#[test]
fn the_dcc_refreshes_the_hardware_monitor() {
    let monitor = FakeHardware::new(vec![UNREAD]);
    let _dcc = reading(&monitor);

    assert!(
        wait_for(5, || monitor.refreshes() >= 1),
        "the DCC refreshed the monitor"
    );
}

/// **The hardware monitor is refreshed at most about once a second.** A
/// sensor read can cost milliseconds: a DCC ticking at 100 Hz for two
/// seconds refreshes the monitor on its first tick and about once a second
/// after, not on every tick.
#[test]
fn the_hardware_monitor_is_refreshed_at_most_about_once_a_second() {
    let monitor = FakeHardware::new(vec![UNREAD]);
    let began = std::time::Instant::now();
    let mut dcc = reading(&monitor);
    thread::sleep(Duration::from_secs(2));
    dcc.stop();
    let elapsed = began.elapsed().as_secs_f64();

    let refreshes = monitor.refreshes();
    // The first tick, one a second after, and one for a tick that lands on
    // the second's edge.
    let at_most = elapsed.floor() as usize + 2;
    assert!(
        refreshes <= at_most,
        "{refreshes} refreshes in {elapsed:.2} s; at most {at_most}"
    );
}

/// **The hardware monitor is read again a second later.** Once a second the
/// DCC refreshes and reads the monitor anew: a monitor that reads `Cool` on
/// its first refresh and `Throttling` on its second has the context end up
/// `Throttling`.
#[test]
fn the_hardware_monitor_is_read_again_a_second_later() {
    let first = (ThermalStatus::Cool, BatteryLevel::High, 0.1);
    let second = (ThermalStatus::Throttling, BatteryLevel::Low, 0.8);
    let monitor = FakeHardware::new(vec![UNREAD, first, second]);
    let dcc = reading(&monitor);

    let read_again = wait_for(5, || same(hardware(&dcc).0, second));
    assert!(
        read_again,
        "the context reads the second refresh: {:?} after {} refreshes",
        hardware(&dcc).0,
        monitor.refreshes()
    );
}

/// **The hardware monitor leaves the GPU load as it is.** It measures no GPU:
/// the GPU load a hardware report set stays once the DCC has read the monitor
/// again.
#[test]
fn the_hardware_monitor_leaves_the_gpu_load_as_it_is() {
    let first = (ThermalStatus::Warm, BatteryLevel::High, 0.3);
    let later = (ThermalStatus::Throttling, BatteryLevel::Low, 0.7);
    let monitor = FakeHardware::new(vec![UNREAD, first, later]);
    let dcc = reading(&monitor);
    assert!(
        wait_for(5, || monitor.refreshes() >= 1),
        "the DCC refreshed the monitor"
    );

    dcc.event_sender()
        .send(report(first, 0.6))
        .expect("the DCC listens");
    assert!(
        wait_for(5, || (hardware(&dcc).1 - 0.6).abs() < 1e-6),
        "the report set the GPU load: {}",
        hardware(&dcc).1
    );
    assert!(
        wait_for(5, || same(hardware(&dcc).0, later)),
        "the DCC read the monitor again: {:?}",
        hardware(&dcc).0
    );

    let gpu = hardware(&dcc).1;
    assert!(
        (gpu - 0.6).abs() < 1e-6,
        "the monitor left the GPU load the report set, got {gpu}"
    );
}

/// **A hardware report still sets the hardware beside a monitor.** The DCC
/// reads its monitor, then the telemetry reports a `Critical` heat, a
/// `Critical` battery and a 0.9 CPU load: the context says what the report
/// said, until the monitor is read again.
#[test]
fn a_hardware_report_still_sets_the_hardware_beside_a_monitor() {
    let read = (ThermalStatus::Cool, BatteryLevel::Mains, 0.1);
    let reported = (ThermalStatus::Critical, BatteryLevel::Critical, 0.9);
    let monitor = FakeHardware::new(vec![UNREAD, read]);
    let dcc = reading(&monitor);
    assert!(
        wait_for(5, || same(hardware(&dcc).0, read)),
        "the DCC read its monitor: {:?}",
        hardware(&dcc).0
    );

    // The monitor is read again about once a second, and then overwrites
    // what the report said: a sample taken while a read of the monitor came
    // in between says nothing, and the report is sent again.
    for (attempt, gpu) in [0.5_f32, 0.55, 0.6].into_iter().enumerate() {
        let before = monitor.refreshes();
        dcc.event_sender()
            .send(report(reported, gpu))
            .expect("the DCC listens");
        let mut seen = None;
        let ingested = wait_for(5, || {
            let (hw, gpu_load) = hardware(&dcc);
            seen = Some(hw);
            (gpu_load - gpu).abs() < 1e-6
        });
        assert!(ingested, "the DCC ingested the report (attempt {attempt})");
        if monitor.refreshes() != before {
            continue;
        }
        let seen = seen.expect("the context was sampled");
        assert!(
            same(seen, reported),
            "the context says what the report said: {seen:?}"
        );
        return;
    }
    panic!("the monitor was read again during every report sent");
}

/// The read of a hardware monitor that one of [`HeldHardware`]'s methods
/// holds until it is let go.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Held {
    Refresh,
    Thermal,
    Battery,
    CpuLoad,
}

/// A hardware monitor whose `held` read, the first time, says it began and
/// waits until it is let go: a sensor that takes its time.
struct HeldHardware {
    held: Held,
    began: crossbeam_channel::Sender<()>,
    let_go: crossbeam_channel::Receiver<()>,
    once: std::sync::atomic::AtomicBool,
}

impl HeldHardware {
    fn hold(&self, read: Held) {
        if read == self.held && !self.once.swap(true, Ordering::SeqCst) {
            let _ = self.began.send(());
            let _ = self.let_go.recv_timeout(Duration::from_secs(10));
        }
    }
}

impl HardwareMonitor for HeldHardware {
    fn thermal_status(&self) -> ThermalStatus {
        self.hold(Held::Thermal);
        ThermalStatus::Warm
    }
    fn battery_level(&self) -> BatteryLevel {
        self.hold(Held::Battery);
        BatteryLevel::Low
    }
    fn cpu_load(&self) -> f32 {
        self.hold(Held::CpuLoad);
        0.42
    }
    fn refresh(&self) {
        self.hold(Held::Refresh);
    }
}

/// **A slow hardware read never holds the context.** The frame's thread
/// reads the context every frame: while the DCC's thread waits on a sensor
/// — refreshing, or reading the heat, the battery or the CPU's load — the
/// context can still be read, and once the read ends the context says it.
#[test]
fn a_slow_hardware_read_never_holds_the_context() {
    for held in [Held::Refresh, Held::Thermal, Held::Battery, Held::CpuLoad] {
        let (began, began_rx) = crossbeam_channel::unbounded();
        let (let_go, let_go_rx) = crossbeam_channel::unbounded();
        let monitor = Arc::new(HeldHardware {
            held,
            began,
            let_go: let_go_rx,
            once: std::sync::atomic::AtomicBool::new(false),
        });
        let (mut dcc, rx) = DccService::new(DccConfig {
            tick_rate: 100,
            ..Default::default()
        });
        dcc.set_hardware_monitor(monitor as Arc<dyn HardwareMonitor>);
        dcc.start(rx);
        began_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|_| panic!("the DCC read the monitor ({held:?})"));

        let context = dcc.context_handle();
        let readable = context.try_read().is_ok();
        let_go.send(()).expect("the monitor waits");
        assert!(
            readable,
            "the context is not held while the monitor's {held:?} read waits"
        );
        assert!(
            wait_for(5, || same(
                hardware(&dcc).0,
                (ThermalStatus::Warm, BatteryLevel::Low, 0.42)
            )),
            "the context says what the read found ({held:?}): {:?}",
            hardware(&dcc).0
        );
        dcc.stop();
    }
}

/// **A hardware monitor given to a running DCC is read.** The DCC's thread
/// looks for its monitor as it ticks: a monitor given after `start` is
/// refreshed and read, and the context says what it read.
#[test]
fn a_hardware_monitor_given_to_a_running_dcc_is_read() {
    let warm = (ThermalStatus::Warm, BatteryLevel::Low, 0.42);
    let (mut dcc, rx) = DccService::new(DccConfig {
        tick_rate: 100,
        ..Default::default()
    });
    dcc.start(rx);
    thread::sleep(Duration::from_millis(50));

    let monitor = FakeHardware::new(vec![UNREAD, warm]);
    dcc.set_hardware_monitor(Arc::clone(&monitor) as Arc<dyn HardwareMonitor>);
    assert!(
        wait_for(5, || same(hardware(&dcc).0, warm)),
        "the context reads the monitor given after start: {:?} after {} refreshes",
        hardware(&dcc).0,
        monitor.refreshes()
    );
}
