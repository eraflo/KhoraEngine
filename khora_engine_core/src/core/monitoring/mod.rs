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

//! Core monitoring types and traits for resource monitoring system.

mod resource_monitor;
mod specialized_monitors;
mod types;

pub use resource_monitor::ResourceMonitor;
pub use specialized_monitors::{GpuMonitor, MemoryMonitor, VramMonitor};
pub use types::*;
