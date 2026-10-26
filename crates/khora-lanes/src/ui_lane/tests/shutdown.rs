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

//! Shutting the UI lane down gives back every GPU object it created.

use super::super::UiRenderLane;

/// The UI lane owns its projection and instance buffers, its sampler and its
/// global and instance bind groups; the layouts and pipeline belong to the
/// `PipelineSystem`.
#[test]
fn ui_shutdown_destroys_every_buffer_it_created() {
    crate::tests::assert_shutdown_destroys_what_it_created(&UiRenderLane::new());
}
