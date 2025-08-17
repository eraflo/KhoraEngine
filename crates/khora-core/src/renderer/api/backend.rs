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

use super::common::{GraphicsAdapterInfo, GraphicsBackendType};
use std::time::Duration;

/// Configuration for backend selection.
#[derive(Debug, Clone)]
pub struct BackendSelectionConfig {
    /// Preferred backends in order of preference
    pub preferred_backends: Vec<GraphicsBackendType>,
    /// Maximum time to spend on backend selection
    pub timeout: Duration,
    /// Whether to prefer discrete GPUs over integrated ones
    pub prefer_discrete_gpu: bool,
}

impl Default for BackendSelectionConfig {
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

/// Result of a backend selection operation.
#[derive(Debug)]
pub struct BackendSelectionResult<TAdapter> {
    /// The selected adapter
    pub adapter: TAdapter,
    /// Information about the selected adapter
    pub adapter_info: GraphicsAdapterInfo,
    /// Time taken for the selection process
    pub selection_time_ms: u64,
    /// All backends that were attempted during selection
    pub attempted_backends: Vec<GraphicsBackendType>,
}
