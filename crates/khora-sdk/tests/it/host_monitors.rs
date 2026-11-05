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

//! The monitors the bootstrap hands out: to the telemetry service, those a
//! host built and put in the runtime, and the engine's own; to the DCC, the
//! hardware monitor its thread reads.

use std::borrow::Cow;
use std::sync::Arc;

use khora_sdk::khora_core::telemetry::monitoring::{ResourceMonitor, ResourceUsageReport};
use khora_sdk::{
    AgentProvider, DccService, EngineApp, EngineCore, GameWorld, HostMonitors, InputEvent,
    MonitorRegistry, MonitoredResourceType, PhaseProvider, Runtime, WindowConfig,
};

/// An application that does nothing: the bootstrap alone is under test.
struct Idle;

impl AgentProvider for Idle {
    fn register_agents(&self, _dcc: &DccService, _runtime: &mut Runtime) {}
}
impl PhaseProvider for Idle {}
impl EngineApp for Idle {
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn new() -> Self {
        Idle
    }
    fn setup(&mut self, _world: &mut GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
}

/// A VRAM monitor a host built, as a renderer's `init` returns it.
#[derive(Debug)]
struct HostVram;

impl ResourceMonitor for HostVram {
    fn monitor_id(&self) -> Cow<'static, str> {
        Cow::Borrowed("host VRAM")
    }
    fn resource_type(&self) -> MonitoredResourceType {
        MonitoredResourceType::Vram
    }
    fn get_usage_report(&self) -> ResourceUsageReport {
        ResourceUsageReport::default()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// The monitors the telemetry service of a bootstrapped `engine` holds.
fn registered(engine: &EngineCore<Idle>) -> Vec<Arc<dyn ResourceMonitor>> {
    engine
        .runtime()
        .resources
        .get::<MonitorRegistry>()
        .expect("the bootstrap publishes the monitor registry")
        .get_all_monitors()
}

/// **A host's monitors reach the telemetry service.** A `HostMonitors`
/// resource put in the runtime before the bootstrap ends up in the
/// `MonitorRegistry`, so its reports flow to the DCC.
#[test]
fn a_hosts_monitors_reach_the_monitor_registry() {
    let mut runtime = Runtime::new();
    runtime
        .resources
        .insert(HostMonitors(vec![Arc::new(HostVram)]));
    let mut engine = EngineCore::<Idle>::new();
    engine.bootstrap(Idle, runtime);

    let monitors = registered(&engine);
    assert!(
        monitors
            .iter()
            .any(|monitor| monitor.monitor_id() == "host VRAM"
                && monitor.resource_type() == MonitoredResourceType::Vram),
        "the host's VRAM monitor is registered: {monitors:?}"
    );
}

/// **The host's monitors leave the runtime once registered.** The
/// `HostMonitors` resource is a hand-over, not a second home: after the
/// bootstrap the runtime no longer holds it.
#[test]
fn the_host_monitors_resource_is_gone_after_the_bootstrap() {
    let mut runtime = Runtime::new();
    runtime
        .resources
        .insert(HostMonitors(vec![Arc::new(HostVram)]));
    let mut engine = EngineCore::<Idle>::new();
    engine.bootstrap(Idle, runtime);

    assert!(
        !engine.runtime().resources.contains::<HostMonitors>(),
        "the hand-over resource was removed"
    );
}

/// **The DCC reads the hardware, not the telemetry.** Reading the sensors
/// costs milliseconds, and the telemetry service runs inside the frame: after
/// the bootstrap the DCC holds a hardware monitor, and the telemetry service
/// holds no `Hardware` monitor (the system-RAM one is still there).
#[test]
fn the_bootstrap_gives_the_dcc_a_hardware_monitor_and_the_telemetry_none() {
    let mut engine = EngineCore::<Idle>::new();
    engine.bootstrap(Idle, Runtime::new());

    let dcc = engine.dcc().expect("the bootstrap starts the DCC");
    assert!(
        dcc.hardware_monitor().is_some(),
        "the DCC was given a hardware monitor"
    );

    let monitors = registered(&engine);
    assert!(
        !monitors
            .iter()
            .any(|monitor| monitor.resource_type() == MonitoredResourceType::Hardware),
        "no hardware monitor is registered in the telemetry: {monitors:?}"
    );
    assert!(
        monitors
            .iter()
            .any(|monitor| monitor.resource_type() == MonitoredResourceType::SystemRam),
        "the system-RAM monitor is: {monitors:?}"
    );
}
