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

//! The outcome of a graphics backend selection.

use super::adapter::GraphicsAdapterInfo;
use crate::renderer::api::device::GraphicsBackendType;

/// The successful result of a backend selection operation.
///
/// This struct contains the chosen adapter handle (`TAdapter`), which is a generic
/// type provided by the specific backend implementation (e.g., `wgpu::Adapter`),
/// along with metadata about the selection process.
#[derive(Debug)]
pub struct BackendSelectionResult<TAdapter> {
    /// The handle to the selected graphics adapter (GPU).
    pub adapter: TAdapter,
    /// Detailed information about the selected adapter.
    pub adapter_info: GraphicsAdapterInfo,
    /// The total time taken for the selection process, in milliseconds.
    pub selection_time_ms: u64,
    /// A list of all backend APIs that were attempted during the selection process.
    pub attempted_backends: Vec<GraphicsBackendType>,
}
