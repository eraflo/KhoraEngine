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

//! The units a round counts in: whole microseconds, the busiest device, a
//! transfer's time over the links, and whether a cost fits an envelope.

use std::time::Duration;

use khora_core::agent::gorna::protocol::Envelope;
use khora_core::lane::cost::{Cost, Device, PerDevice, PerLink};

/// Whether `cost` fits `envelope`, its time and memory on every device.
pub(crate) fn fits_envelope(cost: &Cost, envelope: &Envelope) -> bool {
    Device::ALL.iter().all(|device| {
        device.get(&cost.time) <= device.get(&envelope.time)
            && device.get(&cost.memory) <= device.get(&envelope.memory)
    })
}

/// The time on the busiest device.
pub(crate) fn busiest(time: &PerDevice<Duration>) -> Duration {
    time.cpu.max(time.gpu).max(time.npu).max(time.other)
}

/// The time moving `transfer`'s bytes takes over the links, in µs; a link of
/// rate `0` is not timed.
pub(super) fn transfer_micros(transfer: &PerLink<u64>, rate: &PerLink<u64>) -> u64 {
    let over = |bytes: u64, rate: u64| {
        if rate == 0 {
            0
        } else {
            (u128::from(bytes) * 1_000_000 / u128::from(rate)) as u64
        }
    };
    over(transfer.cpu_to_gpu, rate.cpu_to_gpu) + over(transfer.gpu_to_cpu, rate.gpu_to_cpu)
}

pub(crate) fn micros(time: Duration) -> u64 {
    u64::try_from(time.as_micros()).unwrap_or(u64::MAX)
}

pub(super) fn micros_of_ms(ms: f32) -> u64 {
    (f64::from(ms.max(0.0)) * 1_000.0) as u64
}

/// `values` with `f` applied on every device.
pub(super) fn each<T: Copy, U: Default>(values: &PerDevice<T>, f: impl Fn(T) -> U) -> PerDevice<U> {
    PerDevice {
        cpu: f(values.cpu),
        gpu: f(values.gpu),
        npu: f(values.npu),
        other: f(values.other),
    }
}
