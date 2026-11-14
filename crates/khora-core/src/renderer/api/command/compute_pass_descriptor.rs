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

//! The descriptor of a compute pass, and its timestamp requests.

use crate::renderer::api::device::gpu_hook::GpuHook;

/// A descriptor for a compute pass.
#[derive(Debug, Default)]
pub struct ComputePassDescriptor<'a> {
    /// An optional debug label for the compute pass.
    pub label: Option<&'a str>,
    /// Optional timestamp recording requests for this pass, used for profiling.
    pub timestamp_writes: Option<PassTimestampWrites<'a>>,
}

/// Describes a request to write a timestamp at specific points within a pass.
#[derive(Debug, Default)]
pub struct PassTimestampWrites<'a> {
    /// The abstract hook representing the timestamp to be recorded at the beginning of the pass.
    pub beginning_of_pass_hook: Option<&'a GpuHook>,
    /// The abstract hook representing the timestamp to be recorded at the end of the pass.
    pub end_of_pass_hook: Option<&'a GpuHook>,
}
