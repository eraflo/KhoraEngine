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

//! One test at a time owns a GPU device.
//!
//! Several devices driven at once from several threads crash the Khronos
//! Vulkan validation layer (seen with SDK 1.3.283 on Windows: access
//! violations inside `vulkan_layer_chassis::SetDebugUtilsObjectNameEXT` while
//! `Queue::submit` begins its encoder, and inside buffer and bind-group-layout
//! creation). One device hammered by many threads holds, and so does every
//! device in turn: the engine itself builds one device per process. So a test
//! that builds or drives a device holds [`exclusive_gpu`] for as long as the
//! device is alive — validation and object labels stay on, the concurrency
//! *within* each test is untouched, and the tests that use no GPU still run
//! side by side.

use std::sync::{Mutex, MutexGuard};

static GPU: Mutex<()> = Mutex::new(());

/// The process's turn on the GPU, held until dropped. A test that panicked
/// while holding it gives it back as it unwinds: the turn is still good.
pub(crate) fn exclusive_gpu() -> MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
