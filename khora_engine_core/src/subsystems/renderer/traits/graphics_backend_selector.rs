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

use async_trait::async_trait;
use std::time::Duration;

/// Represents the different types of graphics backends available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphicsBackendType {
    /// Vulkan API
    Vulkan,
    /// DirectX 12 API
    DirectX12,
    /// DirectX 11 API
    DirectX11,
    /// OpenGL API
    OpenGL,
    /// Metal API (macOS/iOS)
    Metal,
    /// WebGL (Web)
    WebGL,
}

/// Information about a graphics adapter.
#[derive(Debug, Clone)]
pub struct GraphicsAdapterInfo {
    /// Human-readable name of the adapter
    pub name: String,
    /// The backend type this adapter uses
    pub backend_type: GraphicsBackendType,
    /// Whether this is a discrete GPU
    pub is_discrete: bool,
    /// Vendor ID (e.g., AMD, NVIDIA, Intel)
    pub vendor_id: Option<u32>,
    /// Device ID
    pub device_id: Option<u32>,
}

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
                        GraphicsBackendType::DirectX12,
                        GraphicsBackendType::DirectX11,
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

/// Generic trait for graphics backend selection.
///
/// This trait allows different graphics APIs (WGPU, DirectX, OpenGL, etc.)
/// to implement their own backend selection logic while maintaining a
/// consistent interface.
#[async_trait]
pub trait GraphicsBackendSelector<TAdapter> {
    /// Error type for backend selection operations
    type Error: std::fmt::Debug + std::fmt::Display + Send + Sync + 'static;

    /// Select the best available graphics backend based on the provided configuration.
    ///
    /// ## Arguments
    /// * `config` - Configuration specifying backend preferences and constraints
    ///
    /// ## Returns
    /// * `Result<BackendSelectionResult<TAdapter>, Self::Error>` - The selection result or an error
    async fn select_backend(
        &self,
        config: &BackendSelectionConfig,
    ) -> Result<BackendSelectionResult<TAdapter>, Self::Error>;

    /// Get information about available adapters for a specific backend type.
    ///
    /// ## Arguments
    /// * `backend_type` - The backend type to query
    ///
    /// ## Returns
    /// * `Result<Vec<GraphicsAdapterInfo>, Self::Error>` - List of available adapters or an error
    async fn list_adapters(
        &self,
        backend_type: GraphicsBackendType,
    ) -> Result<Vec<GraphicsAdapterInfo>, Self::Error>;

    /// Check if a specific backend type is supported on the current platform.
    ///
    /// ## Arguments
    /// * `backend_type` - The backend type to check
    ///
    /// ## Returns
    /// * `bool` - Whether the backend is supported
    fn is_backend_supported(&self, backend_type: GraphicsBackendType) -> bool;
}
