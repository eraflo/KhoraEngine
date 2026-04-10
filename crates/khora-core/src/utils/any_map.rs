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

//! Type-erased data store used by [`FrameContext`].
//!
//! A simple `AnyMap` implementation that stores one value per type.
//! O(1) type-safe lookup via `TypeId`.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

/// A thread-safe type-erased map — one value per type.
pub struct AnyMap {
    data: HashMap<TypeId, Arc<dyn Any + Send + Sync>>,
}

impl AnyMap {
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
        }
    }

    /// Inserts a value, replacing any existing value of the same type.
    pub fn insert<T: Send + Sync + 'static>(&mut self, value: T) {
        self.data
            .insert(TypeId::of::<T>(), Arc::new(value));
    }

    /// Returns a cloned Arc reference to the value of the given type, if present.
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.data
            .get(&TypeId::of::<T>())
            .and_then(|v| v.clone().downcast::<T>().ok())
    }

    /// Returns true if a value of type `T` is stored.
    pub fn contains<T: Send + Sync + 'static>(&self) -> bool {
        self.data.contains_key(&TypeId::of::<T>())
    }

    /// Removes and returns the value of type `T`, if present.
    pub fn remove<T: Send + Sync + 'static>(&mut self) -> Option<Arc<T>> {
        self.data
            .remove(&TypeId::of::<T>())
            .and_then(|v| v.downcast::<T>().ok())
    }
}

impl Default for AnyMap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_and_get() {
        let mut map = AnyMap::new();
        map.insert(42u32);
        assert_eq!(*map.get::<u32>().unwrap(), 42);
    }

    #[test]
    fn test_replace() {
        let mut map = AnyMap::new();
        map.insert("hello");
        map.insert("world");
        assert_eq!(*map.get::<&str>().unwrap(), "world");
    }

    #[test]
    fn test_missing_type() {
        let map = AnyMap::new();
        assert!(map.get::<u32>().is_none());
    }

    #[test]
    fn test_contains() {
        let mut map = AnyMap::new();
        map.insert(true);
        assert!(map.contains::<bool>());
        assert!(!map.contains::<u32>());
    }
}
