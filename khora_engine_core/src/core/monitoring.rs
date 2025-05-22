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

use std::fmt::Debug;
use std::borrow::Cow;

///! This module defines the `Monitoring` trait and related types for monitoring resources in a system.
///! The `Monitoring` trait provides methods to get the current usage and limit of a monitored resource.

/// Corresponds to the type of resource being monitored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MonitoredResourceType {
    Vram,
    SystemRam
}

/// Represents the current usage and limit of a monitored resource.
#[derive(Debug, Clone, Copy, Default)]
pub struct ResourceUsageReport {
    pub current_bytes: u64,
    pub peak_bytes: Option<u64>,
    pub total_capacity_bytes: Option<u64>,
}

/// The `ResourceMonitor` trait provides methods to monitor resources in a system.
/// It includes methods to get the ID of the monitor, the type of resource being monitored,
/// and a report of the current usage and limit of the resource.
pub trait ResourceMonitor: Send + Sync + Debug + 'static {
    fn monitor_id(&self) -> Cow<'static, str>;
    fn resource_type(&self) -> MonitoredResourceType;
    fn get_usage_report(&self) -> ResourceUsageReport;
}