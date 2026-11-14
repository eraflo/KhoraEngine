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

//! One kind of device a cost is counted on.

use serde::{Deserialize, Serialize};

use super::cost::PerDevice;

/// One kind of device: a name for one field of a [`PerDevice`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Device {
    /// The host processor.
    Cpu,
    /// The graphics processor.
    Gpu,
    /// The neural processor.
    Npu,
    /// Any other device a backend brings.
    Other,
}

impl Device {
    /// Every device, in the order of [`PerDevice`]'s fields.
    pub const ALL: [Device; 4] = [Device::Cpu, Device::Gpu, Device::Npu, Device::Other];

    /// This device's value in `values`.
    pub fn get<T: Copy>(self, values: &PerDevice<T>) -> T {
        match self {
            Device::Cpu => values.cpu,
            Device::Gpu => values.gpu,
            Device::Npu => values.npu,
            Device::Other => values.other,
        }
    }

    /// This device's value in `values`, to change it.
    pub fn get_mut<T>(self, values: &mut PerDevice<T>) -> &mut T {
        match self {
            Device::Cpu => &mut values.cpu,
            Device::Gpu => &mut values.gpu,
            Device::Npu => &mut values.npu,
            Device::Other => &mut values.other,
        }
    }
}
