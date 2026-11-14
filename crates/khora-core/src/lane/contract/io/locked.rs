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

//! A runtime entry locked for one record of a lane.

use std::sync::MutexGuard;

/// A runtime entry registered as `Arc<Mutex<T>>`, locked for one record.
///
/// Declared as a lock of the registered `Arc<Mutex<T>>`.
pub struct Locked<'f, T: ?Sized> {
    guard: MutexGuard<'f, T>,
}

impl<'f, T: ?Sized> Locked<'f, T> {
    /// Wraps the guard of a lock taken for one record.
    pub fn new(guard: MutexGuard<'f, T>) -> Self {
        Self { guard }
    }
}

impl<T: ?Sized> std::ops::Deref for Locked<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.guard
    }
}

impl<T: ?Sized> std::ops::DerefMut for Locked<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.guard
    }
}
