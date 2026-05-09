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

//! Lock helpers shared by lanes and backends.
//!
//! Replace `.lock().unwrap()` and `.read()/.write().unwrap()` in hot paths
//! with [`mutex_lock`] / [`read_lock`] / [`write_lock`] so a poisoned
//! lock yields a [`LaneError::LockPoisoned`] (or a [`RenderError`]) instead
//! of crashing the frame.
//!
//! These helpers live in `khora-core` so both `khora-lanes` and
//! `khora-infra` can use them without crossing crate boundaries.

use crate::lane::LaneError;
use crate::renderer::error::{RenderError, ResourceError};
use std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Locks a `Mutex<T>`, mapping poisoning to [`LaneError::LockPoisoned`].
///
/// ```ignore
/// let guard = mutex_lock(&shared, "my-lane.shared")?;
/// ```
pub fn mutex_lock<'a, T: ?Sized>(
    lock: &'a Mutex<T>,
    context: &'static str,
) -> Result<MutexGuard<'a, T>, LaneError> {
    lock.lock().map_err(|_| LaneError::lock_poisoned(context))
}

/// Acquires a read lock, mapping poisoning to [`LaneError::LockPoisoned`].
pub fn read_lock<'a, T: ?Sized>(
    lock: &'a RwLock<T>,
    context: &'static str,
) -> Result<RwLockReadGuard<'a, T>, LaneError> {
    lock.read().map_err(|_| LaneError::lock_poisoned(context))
}

/// Acquires a write lock, mapping poisoning to [`LaneError::LockPoisoned`].
pub fn write_lock<'a, T: ?Sized>(
    lock: &'a RwLock<T>,
    context: &'static str,
) -> Result<RwLockWriteGuard<'a, T>, LaneError> {
    lock.write().map_err(|_| LaneError::lock_poisoned(context))
}

/// Locks a `Mutex<T>`, mapping poisoning to a [`RenderError`].
///
/// Use from GPU-init / GPU-shutdown paths that return
/// `Result<_, RenderError>` (the GPU error type) — the
/// `LaneError`-returning [`mutex_lock`] does not compose with `?`
/// there.
pub fn mutex_lock_render<'a, T: ?Sized>(
    lock: &'a Mutex<T>,
    context: &'static str,
) -> Result<MutexGuard<'a, T>, RenderError> {
    lock.lock().map_err(|_| {
        RenderError::ResourceError(ResourceError::BackendError(format!(
            "{}: lock poisoned",
            context
        )))
    })
}

/// Like [`mutex_lock_render`] but for `RwLock<T>` write access.
pub fn write_lock_render<'a, T: ?Sized>(
    lock: &'a RwLock<T>,
    context: &'static str,
) -> Result<RwLockWriteGuard<'a, T>, RenderError> {
    lock.write().map_err(|_| {
        RenderError::ResourceError(ResourceError::BackendError(format!(
            "{}: lock poisoned",
            context
        )))
    })
}

/// Like [`mutex_lock_render`] but for `RwLock<T>` read access.
pub fn read_lock_render<'a, T: ?Sized>(
    lock: &'a RwLock<T>,
    context: &'static str,
) -> Result<RwLockReadGuard<'a, T>, RenderError> {
    lock.read().map_err(|_| {
        RenderError::ResourceError(ResourceError::BackendError(format!(
            "{}: lock poisoned",
            context
        )))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn mutex_lock_succeeds() {
        let m = Mutex::new(42);
        let guard = mutex_lock(&m, "test").unwrap();
        assert_eq!(*guard, 42);
    }

    #[test]
    fn mutex_lock_returns_lane_error_when_poisoned() {
        let m = Arc::new(Mutex::new(0));
        let m_clone = Arc::clone(&m);
        let _ = std::thread::spawn(move || {
            let _g = m_clone.lock().unwrap();
            panic!("poison the mutex");
        })
        .join();
        let err = mutex_lock(&m, "poisoned").unwrap_err();
        assert!(matches!(err, LaneError::LockPoisoned { context: "poisoned" }));
    }

    #[test]
    fn rwlock_read_succeeds() {
        let l = RwLock::new(7);
        let g = read_lock(&l, "test").unwrap();
        assert_eq!(*g, 7);
    }

    #[test]
    fn rwlock_write_succeeds() {
        let l = RwLock::new(0);
        {
            let mut g = write_lock(&l, "test").unwrap();
            *g = 9;
        }
        assert_eq!(*l.read().unwrap(), 9);
    }
}
