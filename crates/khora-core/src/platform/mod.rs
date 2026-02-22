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

//! Provides abstractions over platform-specific functionalities.
//!
//! This module contains traits and types that define a common, engine-wide interface
//! for interacting with the underlying operating system and its features, such as
//! windowing, input, and filesystem access.

pub mod window;

pub use window::{KhoraWindow, KhoraWindowHandle, WindowHandle};

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

/// Trait for observing the physical state of the host platform.
pub trait HardwareMonitor: Send + Sync {
    /// Returns the current thermal status.
    fn thermal_status(&self) -> ThermalStatus;
    /// Returns the current battery/power level.
    fn battery_level(&self) -> BatteryLevel;
    /// Returns the current overall CPU load (0.0 to 1.0).
    fn cpu_load(&self) -> f32;
}
