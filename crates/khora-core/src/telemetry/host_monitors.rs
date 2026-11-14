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

//! The monitors a host built for the telemetry service.

use std::sync::Arc;

use super::monitoring::ResourceMonitor;

/// The monitors a host built for the telemetry service — the renderer's,
/// say, from `RenderSystem::init`. A host puts them in `runtime.resources`;
/// the SDK's bootstrap registers each in the `MonitorRegistry` and removes
/// the resource.
#[derive(Debug, Default)]
pub struct HostMonitors(pub Vec<Arc<dyn ResourceMonitor>>);
