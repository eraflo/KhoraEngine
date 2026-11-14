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

//! A source of video memory usage statistics.

/// A trait for types that can provide VRAM usage statistics.
/// This is typically implemented by a `GraphicsDevice` or a dedicated monitor in `khora-infra`.
pub trait VramProvider: Send + Sync {
    /// Returns the current VRAM usage in megabytes.
    fn get_vram_usage_mb(&self) -> f32;
    /// Returns the peak VRAM usage in megabytes.
    fn get_vram_peak_mb(&self) -> f32;
    /// Returns the total VRAM capacity in megabytes, if available.
    fn get_vram_capacity_mb(&self) -> Option<f32>;
}
