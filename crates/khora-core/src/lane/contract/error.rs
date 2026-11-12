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
    /// What the lane was given does not hold what it needs.
    InvalidInput {
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
    /// A domain-specific error occurred during execution.
    ExecutionFailed(Box<dyn std::error::Error + Send + Sync>),
    /// A domain-specific error occurred during initialization.
    InitializationFailed(Box<dyn std::error::Error + Send + Sync>),
}

impl fmt::Display for LaneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LaneError::NotInitialized => write!(f, "Lane not initialized"),
            LaneError::InvalidInput { expected, received } => {
                write!(f, "Invalid lane input: expected {expected}, got {received}")
            }
            LaneError::LockPoisoned { context } => {
                write!(f, "Lane lock poisoned: {context}")
            }
            LaneError::MissingResource { kind, key } => {
                write!(f, "Missing {kind} resource: {key}")
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
    /// Convenience constructor for something the lane needs and was not
    /// given — a backend the runtime does not hold.
    pub fn missing(type_name: &'static str) -> Self {
        LaneError::InvalidInput {
            expected: type_name,
            received: "not found".into(),
        }
    }
}
