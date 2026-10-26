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

//! What a level costs one frame, per device and per link.

use std::time::Duration;

/// One value per kind of device.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct PerDevice<T> {
    /// The host processor.
    pub cpu: T,
    /// The graphics processor.
    pub gpu: T,
    /// The neural processor.
    pub npu: T,
    /// Any other device a backend brings.
    pub other: T,
}

/// One value per link between devices.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct PerLink<T> {
    /// Host to graphics processor.
    pub cpu_to_gpu: T,
    /// Graphics processor to host.
    pub gpu_to_cpu: T,
}

/// How far a cost can be trusted, from `0.0` (a guess) to `1.0` (measured).
#[derive(Debug, Clone, Copy, Default, PartialEq, PartialOrd)]
pub struct Confidence(pub f32);

/// What running one level costs this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Cost {
    /// The time spent on each device.
    pub time: PerDevice<Duration>,
    /// The bytes sent on each link.
    pub transfer: PerLink<u64>,
    /// The bytes held on each device.
    pub memory: PerDevice<u64>,
    /// How far the figures can be trusted.
    pub confidence: Confidence,
}
