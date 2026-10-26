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

//! What the devices the engine runs on can do.

/// What the devices can do, read once at boot: a lane whose
/// [`supports`](crate::lane::contract::Lane::supports) says no to these is
/// removed from its family's menu.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeviceCaps {
    /// Whether the graphics device has storage buffers.
    pub storage_buffers: bool,
}
