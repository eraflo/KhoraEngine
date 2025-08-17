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

use crate::renderer::{
    api::{GraphicsAdapterInfo, GraphicsBackendType},
    BackendSelectionConfig, BackendSelectionResult,
};
use async_trait::async_trait;

/// Generic trait for graphics backend selection.
#[async_trait]
pub trait GraphicsBackendSelector<TAdapter> {
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
