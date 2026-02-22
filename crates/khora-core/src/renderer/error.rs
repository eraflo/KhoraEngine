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

//! Defines the hierarchy of error types for the rendering subsystem.

use crate::renderer::api::core::ShaderModuleId;
use crate::renderer::api::pipeline::RenderPipelineId;
use std::fmt;

/// An error related to the creation, loading, or compilation of a shader module.
#[derive(Debug)]
pub enum ShaderError {
    /// An error occurred while trying to load the shader source from a path.
    LoadError {
        /// The path of the file that failed to load.
        path: String,
        /// The underlying I/O or source error.
        source_error: String,
    },
    /// The shader source failed to compile into a backend-specific module.
    CompilationError {
        /// A descriptive label for the shader, if available.
        label: String,
        /// Detailed error messages from the shader compiler.
        details: String,
    },
    /// The requested shader module could not be found.
    NotFound {
        /// The ID of the shader module that was not found.
        id: ShaderModuleId,
    },
    /// The specified entry point (e.g., `vs_main`) is not valid for the shader module.
    InvalidEntryPoint {
        /// The ID of the shader module.
        id: ShaderModuleId,
        /// The entry point name that was not found.
        entry_point: String,
    },
}

impl fmt::Display for ShaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShaderError::LoadError { path, source_error } => {
                write!(
                    f,
                    "Failed to load shader source from '{path}': {source_error}"
                )
            }
            ShaderError::CompilationError { label, details } => {
                write!(f, "Shader compilation failed for '{label}': {details}")
            }
            ShaderError::NotFound { id } => {
                write!(f, "Shader module not found for ID: {id:?}")
            }
            ShaderError::InvalidEntryPoint { id, entry_point } => {
                write!(
                    f,
                    "Invalid entry point '{entry_point}' for shader module {id:?}"
                )
            }
        }
    }
}

impl std::error::Error for ShaderError {}

/// An error related to the creation or management of a graphics pipeline.
#[derive(Debug)]
pub enum PipelineError {
    /// Failed to create a pipeline layout from the provided shader reflection data.
    LayoutCreationFailed(String),
    /// The graphics backend failed to compile the full pipeline state object.
    CompilationFailed {
        /// A descriptive label for the pipeline, if available.
        label: Option<String>,
        /// Detailed error messages from the backend.
        details: String,
    },
    /// A shader module provided for the pipeline was invalid or missing.
    InvalidShaderModuleForPipeline {
        /// The ID of the invalid shader module.
        id: ShaderModuleId,
        /// The label of the pipeline being created.
        pipeline_label: Option<String>,
    },
    /// The specified render pipeline ID is not valid.
    InvalidRenderPipeline {
        /// The ID of the invalid render pipeline.
        id: RenderPipelineId,
    },
    /// The fragment shader stage is present but no entry point was specified.
    MissingEntryPointForFragmentShader {
        /// The label of the pipeline being created.
        pipeline_label: Option<String>,
        /// The ID of the fragment shader module.
        shader_id: ShaderModuleId,
    },
    /// The color target format is not compatible with the pipeline or device.
    IncompatibleColorTarget(String),
    /// The depth/stencil format is not compatible with the pipeline or device.
    IncompatibleDepthStencilFormat(String),
    /// A required graphics feature is not supported by the device.
    FeatureNotSupported(String),
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PipelineError::LayoutCreationFailed(msg) => {
                write!(f, "Pipeline layout creation failed: {msg}")
            }
            PipelineError::CompilationFailed { label, details } => {
                write!(
                    f,
                    "Pipeline compilation failed for '{}': {}",
                    label.as_deref().unwrap_or("Unknown"),
                    details
                )
            }
            PipelineError::InvalidShaderModuleForPipeline { id, pipeline_label } => {
                write!(
                    f,
                    "Invalid shader module {:?} for pipeline '{}'",
                    id,
                    pipeline_label.as_deref().unwrap_or("Unknown")
                )
            }
            PipelineError::InvalidRenderPipeline { id } => {
                write!(f, "Invalid render pipeline ID: {id:?}")
            }
            PipelineError::MissingEntryPointForFragmentShader {
                pipeline_label,
                shader_id,
            } => {
                write!(
                    f,
                    "Missing entry point for fragment shader in pipeline '{}', shader ID: {:?}",
                    pipeline_label.as_deref().unwrap_or("Unknown"),
                    shader_id
                )
            }
            PipelineError::IncompatibleColorTarget(msg) => {
                write!(f, "Incompatible color target format: {msg}")
            }
            PipelineError::IncompatibleDepthStencilFormat(msg) => {
                write!(f, "Incompatible depth/stencil format: {msg}")
            }
            PipelineError::FeatureNotSupported(msg) => {
                write!(f, "Feature not supported: {msg}")
            }
        }
    }
}

impl std::error::Error for PipelineError {}

/// An error related to the creation or use of a GPU resource (buffers, textures, etc.).
#[derive(Debug)]
pub enum ResourceError {
    /// A shader-specific error occurred.
    Shader(ShaderError),
    /// A pipeline-specific error occurred.
    Pipeline(PipelineError),
    /// A generic resource could not be found.
    NotFound,
    /// The handle or ID used to reference a resource is invalid.
    InvalidHandle,
    /// An error originating from the specific graphics backend implementation.
    BackendError(String),
    /// An attempt was made to access a resource out of its bounds (e.g., in a buffer).
    OutOfBounds,
}

impl fmt::Display for ResourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResourceError::Shader(err) => write!(f, "Shader resource error: {err}"),
            ResourceError::Pipeline(err) => write!(f, "Pipeline resource error: {err}"),
            ResourceError::NotFound => write!(f, "Resource not found with ID."),
            ResourceError::InvalidHandle => write!(f, "Invalid resource handle or ID."),
            ResourceError::BackendError(msg) => {
                write!(f, "Backend-specific resource error: {msg}")
            }
            ResourceError::OutOfBounds => {
                write!(f, "Resource access out of bounds.")
            }
        }
    }
}

impl std::error::Error for ResourceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ResourceError::Shader(err) => Some(err),
            _ => None,
        }
    }
}

impl From<ShaderError> for ResourceError {
    fn from(err: ShaderError) -> Self {
        ResourceError::Shader(err)
    }
}

impl From<PipelineError> for ResourceError {
    fn from(err: PipelineError) -> Self {
        ResourceError::Pipeline(err)
    }
}

/// A high-level error that can occur within the main rendering system or graphics device.
#[derive(Debug)]
pub enum RenderError {
    /// An operation was attempted before the rendering system was initialized.
    NotInitialized,
    /// A failure occurred during the initialization of the graphics backend.
    InitializationFailed(String),
    /// Failed to acquire the next frame from the swapchain/surface for rendering.
    SurfaceAcquisitionFailed(String),
    /// A critical, unrecoverable rendering operation failed.
    RenderingFailed(String),
    /// An error occurred while managing a GPU resource.
    ResourceError(ResourceError),
    /// The graphics device was lost (e.g., GPU driver crashed or was updated).
    /// This is a catastrophic error that typically requires reinitialization.
    DeviceLost,
    /// An unexpected or internal error occurred.
    Internal(String),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderError::NotInitialized => {
                write!(f, "The rendering system is not initialized.")
            }
            RenderError::InitializationFailed(msg) => {
                write!(f, "Failed to initialize graphics backend: {msg}")
            }
            RenderError::SurfaceAcquisitionFailed(msg) => {
                write!(f, "Failed to acquire surface for rendering: {msg}")
            }
            RenderError::RenderingFailed(msg) => {
                write!(f, "A critical rendering operation failed: {msg}")
            }
            RenderError::ResourceError(err) => {
                write!(f, "Graphics resource operation failed: {err}")
            }
            RenderError::DeviceLost => write!(
                f,
                "The graphics device was lost and needs to be reinitialized."
            ),
            RenderError::Internal(msg) => {
                write!(f, "An internal or unexpected error occurred: {msg}")
            }
        }
    }
}

impl std::error::Error for RenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RenderError::ResourceError(err) => Some(err),
            _ => None,
        }
    }
}

impl From<ResourceError> for RenderError {
    fn from(err: ResourceError) -> Self {
        RenderError::ResourceError(err)
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::*;
    use crate::renderer::api::core::ShaderModuleId;

    #[test]
    fn shader_error_display() {
        let err = ShaderError::LoadError {
            path: "path/to/shader.wgsl".to_string(),
            source_error: "File not found".to_string(),
        };
        assert_eq!(
            format!("{err}"),
            "Failed to load shader source from 'path/to/shader.wgsl': File not found"
        );

        let err_comp = ShaderError::CompilationError {
            label: "MyShader".to_string(),
            details: "Syntax error at line 5".to_string(),
        };
        assert_eq!(
            format!("{err_comp}"),
            "Shader compilation failed for 'MyShader': Syntax error at line 5"
        );
    }

    #[test]
    fn resource_error_display_wrapping_shader_error() {
        let shader_err = ShaderError::NotFound {
            id: ShaderModuleId(42),
        };
        let res_err: ResourceError = shader_err.into();
        assert_eq!(
            format!("{res_err}"),
            "Shader resource error: Shader module not found for ID: ShaderModuleId(42)"
        );
        assert!(res_err.source().is_some());
    }

    #[test]
    fn render_error_display_wrapping_resource_error() {
        let shader_err = ShaderError::NotFound {
            id: ShaderModuleId(101),
        };
        let res_err: ResourceError = shader_err.into();
        let render_err: RenderError = res_err.into();
        assert_eq!(
            format!("{render_err}"),
            "Graphics resource operation failed: Shader resource error: Shader module not found for ID: ShaderModuleId(101)"
        );
        assert!(render_err.source().is_some());
        assert!(render_err.source().unwrap().source().is_some());
    }
}
