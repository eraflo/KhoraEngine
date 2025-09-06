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

//! Defines the public API for the Goal-Oriented Adaptive Serialization system (SAA-Serialize).
//!
//! This module contains the core data structures that form the contract between the user
//! and the engine's serialization capabilities. The central concept is the [`SerializationGoal`]
//! enum, which allows developers to express their *intent* (e.g., speed, stability, readability)
//! rather than being forced to choose a specific file format.

/// Defines the developer's high-level intention for a serialization operation.
///
/// This enum is the core of the SAA-Serialize system's public API. Instead of
/// specifying a format, the user specifies a goal, and the `SerializationAgent`
/// chooses the best strategy to achieve it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SerializationGoal {
    /// Prioritizes the fastest possible loading time.
    /// This is the ideal choice for production game builds where loading screens
    /// must be minimized. The resulting file may be larger or unreadable.
    FastestLoad,

    /// Prioritizes the smallest possible file size on disk.
    /// This is useful for network transfers, content patches, or game saves
    /// where storage space is a concern. Loading may be slower.
    SmallestFileSize,

    /// Prioritizes human-readability for debugging and version control.
    /// The output will be a text-based format (like RON) that can be easily
    /// inspected and diffed. This is the slowest option.
    HumanReadableDebug,

    /// Prioritizes long-term stability and forward compatibility.
    /// The format used will be decoupled from the engine's internal memory layout,
    /// ensuring the scene file can be loaded by future versions of Khora.
    LongTermStability,
}