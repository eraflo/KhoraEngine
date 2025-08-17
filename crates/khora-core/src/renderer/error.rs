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

use super::api::{RenderPipelineId, ShaderModuleId};
use std::fmt;

/// Errors specific to shader module creation or management.
#[derive(Debug)]
pub enum ShaderError {
    LoadError {
        path: String,
        source_error: String,
    },
    CompilationError {
        label: String,
        details: String,
    },
    NotFound {
        id: ShaderModuleId,
    },
    InvalidEntryPoint {
        id: ShaderModuleId,
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

/// Errors related to graphics pipeline creation or management.
#[derive(Debug)]
pub enum PipelineError {
    LayoutCreationFailed(String),
    CompilationFailed {
        label: Option<String>,
        details: String,
    },
    InvalidShaderModuleForPipeline {
        id: ShaderModuleId,
        pipeline_label: Option<String>,
    },
    InvalidRenderPipeline {
        id: RenderPipelineId,
    },
    MissingEntryPointForFragmentShader {
        pipeline_label: Option<String>,
        shader_id: ShaderModuleId,
    },
    IncompatibleColorTarget(String),
    IncompatibleDepthStencilFormat(String),
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

/// Errors related to graphics resource management.
#[derive(Debug)]
pub enum ResourceError {
    Shader(ShaderError),
    Pipeline(PipelineError),
    NotFound,
    InvalidHandle,
    BackendError(String),
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

/// General errors that can occur within the rendering system or graphics device.
#[derive(Debug)]
pub enum RenderError {
    InitializationFailed(String),
    SurfaceAcquisitionFailed(String),
    RenderingFailed(String),
    ResourceError(ResourceError),
    DeviceLost,
    Internal(String),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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
    use crate::renderer::api::ShaderModuleId;

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
