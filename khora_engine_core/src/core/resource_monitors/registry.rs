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

//! Resource Registry
//!
//! Provides a centralized registry for resource monitors that allows
//! subsystems to register their monitors without the engine needing
//! to know about specific implementations.

use crate::core::monitoring::ResourceMonitor;
use std::sync::{Arc, Mutex};

/// Type alias for the resource monitor registry to reduce complexity
type ResourceMonitorRegistry = Arc<Mutex<Vec<Arc<dyn ResourceMonitor>>>>;

/// A global registry for resource monitors
/// This allows subsystems to register their monitors without tight coupling
static RESOURCE_REGISTRY: Mutex<Option<ResourceMonitorRegistry>> = Mutex::new(None);

/// Initialize the global resource registry
pub fn initialize_resource_registry() {
    let mut registry = RESOURCE_REGISTRY.lock().unwrap();
    if registry.is_none() {
        *registry = Some(Arc::new(Mutex::new(Vec::new())));
        log::info!("Resource registry initialized");
    }
}

/// Register a resource monitor in the global registry
pub fn register_resource_monitor(monitor: Arc<dyn ResourceMonitor>) {
    let registry = RESOURCE_REGISTRY.lock().unwrap();
    if let Some(ref monitors) = *registry {
        let mut monitors_guard = monitors.lock().unwrap();
        let monitor_id = monitor.monitor_id().to_string();
        monitors_guard.push(monitor);
        log::info!("Registered resource monitor: {monitor_id}");
    } else {
        let monitor_id = monitor.monitor_id();
        log::error!("Resource registry not initialized! Cannot register monitor: {monitor_id}");
    }
}

/// Get all registered resource monitors
pub fn get_registered_monitors() -> Vec<Arc<dyn ResourceMonitor>> {
    let registry = RESOURCE_REGISTRY.lock().unwrap();
    if let Some(ref monitors) = *registry {
        let monitors_guard = monitors.lock().unwrap();
        monitors_guard.clone()
    } else {
        log::warn!("Resource registry not initialized! Returning empty monitor list");
        Vec::new()
    }
}

/// Clear all registered monitors (for cleanup)
pub fn clear_resource_registry() {
    let registry = RESOURCE_REGISTRY.lock().unwrap();
    if let Some(ref monitors) = *registry {
        let mut monitors_guard = monitors.lock().unwrap();
        monitors_guard.clear();
        log::info!("Resource registry cleared");
    }
}
