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

//! Execution phases for the frame pipeline.
//!
//! Each phase represents a stage in the frame execution order.
//! Built-in phases have fixed IDs 0-5. Custom phases can be created
//! with IDs 6-254 and inserted at any position via the Scheduler API.

/// A phase in the frame execution pipeline.
///
/// Phases are executed in order. Built-in phases have fixed IDs 0-5.
/// Custom phases can be inserted at any position via the Scheduler API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExecutionPhase(u8);

impl ExecutionPhase {
    // Built-in phases — default order
    /// Setup frame, reset state.
    pub const INIT: Self = Self(0);
    /// Read-only: extraction, analysis, observation.
    pub const OBSERVE: Self = Self(1);
    /// Simulation: physics, AI, logic transformations.
    pub const TRANSFORM: Self = Self(2);
    /// Write: sync results, update state, mutations.
    pub const MUTATE: Self = Self(3);
    /// External output: render, audio, UI.
    pub const OUTPUT: Self = Self(4);
    /// Cleanup, telemetry, end-of-frame tasks.
    pub const FINALIZE: Self = Self(5);

    /// Default phase order — used if no custom order is set.
    pub const DEFAULT_ORDER: &'static [Self] = &[
        Self::INIT,
        Self::OBSERVE,
        Self::TRANSFORM,
        Self::MUTATE,
        Self::OUTPUT,
        Self::FINALIZE,
    ];

    /// Create a custom phase. IDs 6-254 are available for user phases.
    /// ID 255 is reserved.
    ///
    /// # Panics
    /// Panics if `id` is not in range 6..255.
    #[must_use]
    pub const fn custom(id: u8) -> Self {
        assert!(
            id > 5 && id < 255,
            "Custom phase IDs must be in range 6..255"
        );
        Self(id)
    }

    /// Returns the raw ID of this phase.
    #[must_use]
    pub const fn id(&self) -> u8 {
        self.0
    }

    /// Returns true if this is a built-in phase.
    #[must_use]
    pub const fn is_builtin(&self) -> bool {
        self.0 <= 5
    }
}

impl std::fmt::Display for ExecutionPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            0 => write!(f, "Init"),
            1 => write!(f, "Observe"),
            2 => write!(f, "Transform"),
            3 => write!(f, "Mutate"),
            4 => write!(f, "Output"),
            5 => write!(f, "Finalize"),
            _ => write!(f, "Custom({})", self.0),
        }
    }
}
