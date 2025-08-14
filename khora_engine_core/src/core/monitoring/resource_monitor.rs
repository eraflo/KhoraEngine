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

//! Core resource monitor trait definition.

use std::borrow::Cow;
use std::fmt::Debug;

use super::types::{MonitoredResourceType, ResourceUsageReport};

/// Core trait for resource monitors.
///
/// Each monitor is responsible for tracking a specific resource type
/// and providing a unified interface for accessing resource usage data.
pub trait ResourceMonitor: Send + Sync + Debug + 'static {
    /// Unique identifier for this monitor instance.
    fn monitor_id(&self) -> Cow<'static, str>;

    /// Type of resource being monitored.
    fn resource_type(&self) -> MonitoredResourceType;

    /// Get general resource usage information.
    fn get_usage_report(&self) -> ResourceUsageReport;

    /// Update the monitor's internal state/statistics.
    /// Default implementation does nothing for monitors that don't need updates.
    fn update(&self) {
        // Default: no-op
    }
}
