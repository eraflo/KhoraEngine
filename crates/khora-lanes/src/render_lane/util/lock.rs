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

//! Render-lane locking helpers.
//!
//! These helpers were promoted into [`khora_core::lane::lock`] so both
//! `khora-lanes` and `khora-infra` (which depend only on `khora-core`)
//! can use them. This file is kept as a thin re-export to preserve the
//! existing `crate::render_lane::util::lock::*` import paths.

pub use khora_core::lane::lock::{
    mutex_lock, mutex_lock_render, read_lock, read_lock_render, write_lock, write_lock_render,
};
