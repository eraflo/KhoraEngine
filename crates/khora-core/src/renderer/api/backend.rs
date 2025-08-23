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

//! Defines data structures for graphics backend selection and introspection.

use super::common::{GraphicsAdapterInfo, GraphicsBackendType};
use std::time::Duration;

/// Configuration for the graphics backend selection process.
///
/// This struct is passed to the [`GraphicsBackendSelector`] to guide its decision on
/// which GPU and API to use for rendering.
#[derive(Debug, Clone)]
pub struct BackendSelectionConfig {
    /// An ordered list of preferred graphics APIs. The selector will try them in this order.
    pub preferred_backends: Vec<GraphicsBackendType>,
    /// The maximum time allowed for the entire adapter discovery and selection process.
    pub timeout: Duration,
    /// If `true`, the selector will prioritize discrete (dedicated) GPUs over integrated ones.
    pub prefer_discrete_gpu: bool,
}

impl Default for BackendSelectionConfig {
    /// Creates a default `BackendSelectionConfig` with sensible, platform-specific preferences.
    ///
    /// The default order is generally Vulkan > DirectX12 > Metal > OpenGL, depending on the OS.
    /// It also defaults to preferring a discrete GPU.
    fn default() -> Self {
        Self {
            preferred_backends: {
                #[cfg(target_os = "windows")]
                {
                    vec![
                        GraphicsBackendType::Vulkan,
                        GraphicsBackendType::Dx12,
                        GraphicsBackendType::Dx11,
                        GraphicsBackendType::OpenGL,
                    ]
                }
                #[cfg(target_os = "macos")]
                {
                    vec![
                        GraphicsBackendType::Metal,
                        GraphicsBackendType::Vulkan,
                        GraphicsBackendType::OpenGL,
                    ]
                }
                #[cfg(target_os = "linux")]
                {
                    vec![GraphicsBackendType::Vulkan, GraphicsBackendType::OpenGL]
                }
                #[cfg(target_arch = "wasm32")]
                {
                    vec![GraphicsBackendType::WebGL]
                }
            },
            timeout: Duration::from_secs(5),
            prefer_discrete_gpu: true,
        }
    }
}

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
