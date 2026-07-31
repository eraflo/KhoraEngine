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

//! Internal helpers for render-lane implementations.

pub mod lock;

pub use lock::{mutex_lock, read_lock, write_lock};

/// Lock helper for callers that cannot propagate `Result<_, LaneError>`.
///
/// Returns the guard on success; on poisoning logs an error and
/// `return`s from the surrounding function. The optional second form
/// returns a caller-provided default value (for non-`()` returning
/// functions).
///
/// ```ignore
/// let mut res = lock_or_log!(self.gpu_resources.lock(), "MyLane::render");
/// let assets = lock_or_log!(meshes.read(), "MyLane::cost", 0.0);
/// ```
#[macro_export]
macro_rules! lock_or_log {
    ($expr:expr, $ctx:literal) => {
        match $expr {
            Ok(g) => g,
            Err(_) => {
                log::error!("{}: lock poisoned", $ctx);
                return;
            }
        }
    };
    ($expr:expr, $ctx:literal, $default:expr) => {
        match $expr {
            Ok(g) => g,
            Err(_) => {
                log::error!("{}: lock poisoned", $ctx);
                return $default;
            }
        }
    };
}
