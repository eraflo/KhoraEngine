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

//! High-level situational model of the engine and hardware.

/// Represents the thermal state of the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThermalStatus {
    /// Device is running cool.
    #[default]
    Cool,
    /// Device is warming up but within normal bounds.
    Warm,
    /// Device is actively throttling performance to shed heat.
    Throttling,
    /// Device is at critical temperature, emergency measures required.
    Critical,
}

/// Represents the power source and battery level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BatteryLevel {
    /// Device is connected to a stable power source.
    #[default]
    Mains,
    /// Battery is high (e.g. > 50%).
    High,
    /// Battery is low (e.g. < 20%).
    Low,
    /// Battery is at critical level, power saving is mandatory.
    Critical,
}

/// Hardware context observed by the DCC.
#[derive(Debug, Clone, Default)]
pub struct HardwareState {
    /// Current thermal status.
    pub thermal: ThermalStatus,
    /// Current battery/power status.
    pub battery: BatteryLevel,
    /// Overall CPU load (0.0 to 1.0).
    pub cpu_load: f32,
    /// Overall GPU load (0.0 to 1.0).
    pub gpu_load: f32,
}

/// The high-level workload state of the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExecutionPhase {
    /// Engine is starting up and loading assets.
    #[default]
    Boot,
    /// User is in an interactive menu.
    Menu,
    /// Full simulation is running (gameplay).
    Simulation,
    /// Application is minimized or lost focus.
    Background,
}

/// The complete context model used for strategic decision making.
#[derive(Debug, Clone, Default)]
pub struct Context {
    /// Observed hardware state.
    pub hardware: HardwareState,
    /// Current engine execution phase.
    pub phase: ExecutionPhase,
}
