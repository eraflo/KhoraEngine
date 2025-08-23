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

/// A trait for a system that discovers and selects a suitable graphics backend.
///
/// This trait abstracts the logic for initializing a graphics context, querying the
/// available adapters (GPUs), and selecting the best one based on a given configuration.
/// Since adapter enumeration can be a slow I/O operation, the primary methods are
/// asynchronous.
///
/// A concrete implementation of this trait will live in `khora-infra` and will typically
/// wrap a library like `wgpu`.
#[async_trait]
pub trait GraphicsBackendSelector<TAdapter> {
    /// The error type returned if backend selection fails.
    type Error: std::fmt::Debug + std::fmt::Display + Send + Sync + 'static;

    /// Asynchronously selects the best available graphics adapter based on the provided configuration.
    ///
    /// This method will attempt to honor the preferences in the `config` (e.g.,
    /// prefer a discrete GPU, or a specific backend API like Vulkan), falling back
    /// to other options if the preferred ones are not available.
    ///
    /// # Arguments
    ///
    /// * `config`: A [`BackendSelectionConfig`] specifying backend preferences and constraints.
    ///
    /// # Returns
    ///
    /// A `Result` containing a [`BackendSelectionResult`] with the chosen adapter
    /// and instance, or an error if no suitable adapter could be found.
    async fn select_backend(
        &self,
        config: &BackendSelectionConfig,
    ) -> Result<BackendSelectionResult<TAdapter>, Self::Error>;

    /// Asynchronously queries and lists all available adapters for a specific backend API.
    ///
    /// # Arguments
    ///
    /// * `backend_type`: The specific backend API (e.g., Vulkan, Dx12) to query.
    ///
    /// # Returns
    ///
    /// A `Result` containing a `Vec` of [`GraphicsAdapterInfo`] for all compatible
    /// adapters, or an error if the backend API is not available.
    async fn list_adapters(
        &self,
        backend_type: GraphicsBackendType,
    ) -> Result<Vec<GraphicsAdapterInfo>, Self::Error>;

    /// Synchronously checks if a specific backend API is supported on the current platform.
    ///
    /// # Arguments
    ///
    /// * `backend_type`: The backend API to check.
    ///
    /// # Returns
    ///
    /// `true` if the backend is likely to be supported, `false` otherwise.
    fn is_backend_supported(&self, backend_type: GraphicsBackendType) -> bool;
}
