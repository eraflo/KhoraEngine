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

//! A report of video memory usage.

/// A report of Video RAM (VRAM) usage.
#[derive(Debug, Clone, Copy, Default)]
pub struct VramReport {
    /// The number of bytes of VRAM currently in use.
    pub current_usage_bytes: usize,
    /// The peak number of bytes of VRAM ever in use, if tracked.
    pub peak_usage_bytes: Option<usize>,
    /// The total physical VRAM capacity in bytes, if available.
    pub total_capacity_bytes: Option<usize>,
}
