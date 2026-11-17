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

//! [`RwLockExt`]: reading and writing an `RwLock` through its poison.

use std::sync::{PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// `RwLock` access that recovers a poisoned lock and goes on.
///
/// Use it only where a writer that panicked leaves the value whole, so
/// reading it after the panic is sound.
pub trait RwLockExt<T: ?Sized> {
    /// Locks for reading, recovering the guard if a panicking writer poisoned the lock.
    ///
    /// # Panics
    ///
    /// Panics if the current thread already holds the write lock, as
    /// `RwLock::read` may.
    fn read_recovered(&self) -> RwLockReadGuard<'_, T>;

    /// Locks for writing, recovering the guard if a panicking writer poisoned the lock.
    ///
    /// # Panics
    ///
    /// Panics if the current thread already holds the lock, as
    /// `RwLock::write` may.
    fn write_recovered(&self) -> RwLockWriteGuard<'_, T>;
}

impl<T: ?Sized> RwLockExt<T> for RwLock<T> {
    fn read_recovered(&self) -> RwLockReadGuard<'_, T> {
        self.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write_recovered(&self) -> RwLockWriteGuard<'_, T> {
        self.write().unwrap_or_else(PoisonError::into_inner)
    }
}
