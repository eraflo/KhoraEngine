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

//! A generic, type-safe service locator for engine subsystems.
//!
//! The [`ServiceRegistry`] provides a type-map where agents can store and
//! retrieve shared references to services (e.g., `GraphicsDevice`,
//! `RenderSystem`) without coupling the [`EngineContext`](crate::EngineContext)
//! to any specific subsystem.
//!
//! # Design
//!
//! This follows the **Service Locator** pattern to satisfy the
//! **Interface Segregation Principle**: each agent fetches only the services
//! it needs, and adding new services never modifies `EngineContext`.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

/// A generic service registry keyed by [`TypeId`].
///
/// Services are stored as `Arc<dyn Any + Send + Sync>` and can be retrieved
/// by their concrete type via [`get`](ServiceRegistry::get).
///
/// An optional `parent` registry enables per-frame overlays: a frame-level
/// registry can hold only frame-specific services (`FrameContext`, viewport
/// target, …) while delegating every other lookup to the engine-level
/// registry via the parent chain.
///
/// # Example
///
/// ```rust
/// use khora_core::service_registry::ServiceRegistry;
///
/// struct MyService { value: i32 }
///
/// let mut registry = ServiceRegistry::new();
/// registry.insert(MyService { value: 42 });
///
/// let svc = registry.get::<MyService>().unwrap();
/// assert_eq!(svc.value, 42);
/// ```
#[derive(Default)]
pub struct ServiceRegistry {
    services: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    /// Optional parent registry.  Lookups fall through to the parent when
    /// a service is not found in the local map.
    parent: Option<Arc<ServiceRegistry>>,
}

impl ServiceRegistry {
    /// Creates an empty service registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            services: HashMap::new(),
            parent: None,
        }
    }

    /// Creates a frame-level registry that delegates unknown lookups to
    /// `parent`.  Local inserts shadow parent entries of the same type.
    #[must_use]
    pub fn with_parent(parent: Arc<ServiceRegistry>) -> Self {
        Self {
            services: HashMap::new(),
            parent: Some(parent),
        }
    }

    /// Inserts a service into the registry, keyed by `T`'s [`TypeId`].
    ///
    /// If a service of the same type was already registered, it is replaced.
    pub fn insert<T: Send + Sync + 'static>(&mut self, service: T) {
        self.services.insert(TypeId::of::<T>(), Box::new(service));
    }

    /// Retrieves a shared reference to a previously registered service.
    ///
    /// Checks the local registry first, then walks up the parent chain.
    /// Returns `None` if the service is absent from the entire chain.
    #[must_use]
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.services
            .get(&TypeId::of::<T>())
            .and_then(|boxed| boxed.downcast_ref::<T>())
            .or_else(|| self.parent.as_deref()?.get::<T>())
    }

    /// Returns `true` if a service of type `T` is registered in this
    /// registry or any ancestor.
    #[must_use]
    pub fn contains<T: Send + Sync + 'static>(&self) -> bool {
        self.services.contains_key(&TypeId::of::<T>())
            || self
                .parent
                .as_deref()
                .map(|p| p.contains::<T>())
                .unwrap_or(false)
    }

    /// Returns the number of registered services.
    #[must_use]
    pub fn len(&self) -> usize {
        self.services.len()
    }

    /// Returns `true` if no services are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.services.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeDevice {
        name: String,
    }

    struct FakeRenderer {}

    #[test]
    fn test_insert_and_get() {
        let mut registry = ServiceRegistry::new();
        let device = FakeDevice {
            name: "GPU-0".to_string(),
        };
        registry.insert(device);

        let retrieved = registry.get::<FakeDevice>().unwrap();
        assert_eq!(retrieved.name, "GPU-0");
    }

    #[test]
    fn test_get_missing_returns_none() {
        let registry = ServiceRegistry::new();
        assert!(registry.get::<FakeDevice>().is_none());
    }

    #[test]
    fn test_multiple_services() {
        let mut registry = ServiceRegistry::new();
        registry.insert(FakeDevice {
            name: "GPU".to_string(),
        });
        registry.insert(FakeRenderer {});

        assert_eq!(registry.len(), 2);
        assert!(registry.contains::<FakeDevice>());
        assert!(registry.contains::<FakeRenderer>());
    }

    #[test]
    fn test_replace_service() {
        let mut registry = ServiceRegistry::new();
        registry.insert(FakeDevice {
            name: "old".to_string(),
        });
        registry.insert(FakeDevice {
            name: "new".to_string(),
        });

        let retrieved = registry.get::<FakeDevice>().unwrap();
        assert_eq!(retrieved.name, "new");
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn test_default_is_empty() {
        let registry = ServiceRegistry::default();
        assert!(registry.is_empty());
    }
}
