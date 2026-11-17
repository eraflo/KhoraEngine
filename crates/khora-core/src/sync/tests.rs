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

use std::panic::{self, AssertUnwindSafe};
use std::sync::{Mutex, RwLock};

use super::{LockExt, RwLockExt};

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

/// Panics while writing `lock` after storing `value`, leaving it poisoned.
fn poison_rw_lock(lock: &RwLock<u32>, value: u32) {
    let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
        let mut guard = lock.write().expect("not yet poisoned");
        *guard = value;
        panic!("a writer panics");
    }));
    assert!(outcome.is_err());
    assert!(lock.is_poisoned());
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

#[test]
fn read_recovered_reads_the_value_a_panicking_writer_left() {
    let lock = RwLock::new(1);
    poison_rw_lock(&lock, 7);
    assert_eq!(*lock.read_recovered(), 7);
}

#[test]
fn write_recovered_writes_through_the_poison() {
    let lock = RwLock::new(1);
    poison_rw_lock(&lock, 7);
    *lock.write_recovered() = 9;
    assert_eq!(*lock.read_recovered(), 9);
}

#[test]
fn rw_lock_recovered_reads_and_writes_a_healthy_lock() {
    let lock = RwLock::new(3);
    *lock.write_recovered() += 1;
    assert_eq!(*lock.read_recovered(), 4);
    assert!(!lock.is_poisoned());
}
