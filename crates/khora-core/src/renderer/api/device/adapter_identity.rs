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

//! What identifies a GPU adapter: its vendor, device and driver.

use serde::{Deserialize, Serialize};

/// What identifies the GPU adapter beyond its name: who made it, which
/// device, which driver.
///
/// Two machines whose adapters differ by any of these fields can cost a
/// frame differently — a driver update alone can — so each field takes part
/// in a machine's key ([`MachineIdentity::key`](crate::platform::MachineIdentity::key)).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AdapterIdentity {
    /// The name of the adapter (e.g., "NVIDIA GeForce RTX 4090").
    pub name: String,
    /// The backend-specific vendor id (generally the PCI vendor id).
    pub vendor: u32,
    /// The backend-specific device id (generally the PCI device id).
    pub device: u32,
    /// The driver's name.
    pub driver: String,
    /// What the driver says of itself, typically its version.
    pub driver_info: String,
}
