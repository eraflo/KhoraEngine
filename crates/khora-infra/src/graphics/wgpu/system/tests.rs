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

//! The render system's thread contract.

use super::*;

fn assert_send_sync<T: Send + Sync>() {}

/// **The render system is `Send + Sync` by its fields.**
///
/// The engine hands it across threads, and no `unsafe impl` vouches for
/// it: this compiles only while every field is `Send + Sync`.
#[test]
fn the_render_system_is_send_and_sync() {
    assert_send_sync::<WgpuRenderSystem>();
}

/// **A requested viewport size waits for the frame to settle it.**
///
/// The request changes nothing by itself; the debounce hands it out at the
/// next poll, the target being smaller.
#[test]
fn request_viewport_size_hands_the_size_to_the_next_settle() {
    let mut system = WgpuRenderSystem::new();

    system.request_viewport_size(640, 480);

    assert_eq!(
        system.viewport_size(),
        (0, 0),
        "the target is not rebuilt by the request"
    );
    assert_eq!(
        system
            .viewport_resize
            .poll((0, 0), std::time::Instant::now()),
        Some((640, 480))
    );
}
