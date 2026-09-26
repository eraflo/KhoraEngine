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

//! What a lane reports when it cannot run.

use std::fmt;

/// Error type for lane operations.
#[derive(Debug)]
pub enum LaneError {
    /// The lane has not been initialized yet.
    NotInitialized,
    /// The execution context passed to the lane has the wrong type.
    InvalidContext {
        /// What the lane expected.
        expected: &'static str,
        /// Description of what was received.
        received: String,
    },
    /// A `RwLock` / `Mutex` was poisoned by a prior panic.
    LockPoisoned {
        /// Which lock — used in the error message for diagnostics.
        context: &'static str,
    },
    /// A required GPU resource (pipeline, buffer, texture, sampler) was
    /// not present in the registry the lane consulted.
    MissingResource {
        /// What kind of resource (e.g. `"pipeline"`, `"buffer"`).
        kind: &'static str,
        /// Human-readable key the lane looked up.
        key: String,
    },
    /// A required asset was not present in the registry / cache.
    MissingAsset {
        /// Asset kind (`"mesh"`, `"texture"`, …).
        kind: &'static str,
        /// Stringified asset id (UUID, name, …).
        id: String,
    },
    /// A domain-specific error occurred during execution.
    ExecutionFailed(Box<dyn std::error::Error + Send + Sync>),
    /// A domain-specific error occurred during initialization.
    InitializationFailed(Box<dyn std::error::Error + Send + Sync>),
}

impl fmt::Display for LaneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LaneError::NotInitialized => write!(f, "Lane not initialized"),
            LaneError::InvalidContext { expected, received } => {
                write!(
                    f,
                    "Invalid lane context: expected {expected}, got {received}"
                )
            }
            LaneError::LockPoisoned { context } => {
                write!(f, "Lane lock poisoned: {context}")
            }
            LaneError::MissingResource { kind, key } => {
                write!(f, "Missing {kind} resource: {key}")
            }
            LaneError::MissingAsset { kind, id } => {
                write!(f, "Missing {kind} asset: {id}")
            }
            LaneError::ExecutionFailed(e) => write!(f, "Lane execution failed: {e}"),
            LaneError::InitializationFailed(e) => write!(f, "Lane initialization failed: {e}"),
        }
    }
}

impl std::error::Error for LaneError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LaneError::ExecutionFailed(e) | LaneError::InitializationFailed(e) => Some(e.as_ref()),
            _ => None,
        }
    }
}

impl LaneError {
    /// Convenience constructor for a missing context entry.
    pub fn missing(type_name: &'static str) -> Self {
        LaneError::InvalidContext {
            expected: type_name,
            received: "not found in LaneContext".into(),
        }
    }

    /// Convenience constructor for a poisoned lock.
    pub fn lock_poisoned(context: &'static str) -> Self {
        LaneError::LockPoisoned { context }
    }

    /// Convenience constructor for a missing resource.
    pub fn missing_resource(kind: &'static str, key: impl Into<String>) -> Self {
        LaneError::MissingResource {
            kind,
            key: key.into(),
        }
    }

    /// Convenience constructor for a missing asset.
    pub fn missing_asset(kind: &'static str, id: impl std::fmt::Display) -> Self {
        LaneError::MissingAsset {
            kind,
            id: id.to_string(),
        }
    }
}
