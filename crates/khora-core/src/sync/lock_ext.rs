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

//! [`LockExt`]: locking a `Mutex` through its poison.

use std::sync::{Mutex, MutexGuard, PoisonError};

/// A `Mutex` lock that recovers a poisoned lock and goes on.
///
/// Use it only where a holder that panicked leaves the value whole, so
/// reading it after the panic is sound.
pub trait LockExt<T: ?Sized> {
    /// Locks the mutex, recovering the guard if a panicking holder poisoned it.
    ///
    /// # Panics
    ///
    /// Panics if the current thread already holds the lock, as
    /// `Mutex::lock` may.
    fn lock_recovered(&self) -> MutexGuard<'_, T>;
}

impl<T: ?Sized> LockExt<T> for Mutex<T> {
    fn lock_recovered(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{self, AssertUnwindSafe};
    use std::sync::Mutex;

    use super::LockExt;

    /// Panics while holding `mutex` after writing `value`, leaving it poisoned.
    fn poison_mutex(mutex: &Mutex<u32>, value: u32) {
        let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
            let mut guard = mutex.lock().expect("not yet poisoned");
            *guard = value;
            panic!("a holder panics");
        }));
        assert!(outcome.is_err());
        assert!(mutex.is_poisoned());
    }

    #[test]
    fn lock_recovered_reads_the_value_a_panicking_holder_left() {
        let mutex = Mutex::new(1);
        poison_mutex(&mutex, 7);
        assert_eq!(*mutex.lock_recovered(), 7);
    }

    #[test]
    fn lock_recovered_writes_through_the_poison() {
        let mutex = Mutex::new(1);
        poison_mutex(&mutex, 7);
        *mutex.lock_recovered() = 9;
        assert_eq!(*mutex.lock_recovered(), 9);
    }

    #[test]
    fn lock_recovered_locks_a_healthy_mutex() {
        let mutex = Mutex::new(3);
        *mutex.lock_recovered() += 1;
        assert_eq!(*mutex.lock_recovered(), 4);
        assert!(!mutex.is_poisoned());
    }
}
