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

//! Defines the abstract contract for serialization strategies and their associated types.
//!
//! The core of this module is the [`SerializationStrategy`] trait, which provides
//! a unified interface for all serialization `Lanes`. This allows the `SerializationAgent`
//! to manage and dispatch tasks to different strategies polymorphically.

use khora_data::ecs::World;
use std::fmt;

/// An error that can occur during the serialization process.
#[derive(Debug)]
pub enum SerializationError {
    /// Indicates a failure during I/O or data conversion.
    ProcessingFailed(String),
}

/// An error that can occur during the deserialization process.
#[derive(Debug)]
pub enum DeserializationError {
    /// The data is corrupted or does not match the expected format.
    InvalidFormat(String),
    /// A failure occurred while populating the world.
    WorldPopulationFailed(String),
}

impl fmt::Display for SerializationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SerializationError::ProcessingFailed(msg) => {
                write!(f, "Serialization failed: {}", msg)
            }
        }
    }
}

impl fmt::Display for DeserializationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeserializationError::InvalidFormat(msg) => {
                write!(f, "Deserialization failed: Invalid format - {}", msg)
            }
            DeserializationError::WorldPopulationFailed(msg) => {
                write!(f, "Deserialization failed: World population - {}", msg)
            }
        }
    }
}

/// The abstract contract for a scene serialization strategy `Lane`.
///
/// Each concrete implementation of this trait represents a different method
/// of converting a `World` to and from a persistent format.
pub trait SerializationStrategy: Send + Sync {
    /// Returns the unique, versioned string identifier for this strategy.
    ///
    /// This ID is written to the `SceneHeader` and used by the `SerializationAgent`
    /// to look up the correct strategy from its registry during deserialization.
    /// Example: `"KH_RECIPE_V1"`.
    fn get_strategy_id(&self) -> &'static str;

    /// Serializes the given `World` into a byte payload.
    ///
    /// # Arguments
    /// * `world` - A reference to the world to be serialized.
    ///
    /// # Returns
    /// A `Result` containing the binary payload `Vec<u8>` or a `SerializationError`.
    fn serialize(&self, world: &World) -> Result<Vec<u8>, SerializationError>;

    /// Deserializes data from a byte payload to populate the given `World`.
    ///
    /// This method should assume the `SceneHeader` has already been parsed and validated,
    /// and that `data` is the correct payload for this strategy.
    ///
    /// # Arguments
    /// * `data` - The raw byte payload to deserialize.
    /// * `world` - A mutable reference to the world to be populated.
    ///
    /// # Returns
    /// A `Result` indicating success or a `DeserializationError`.
    fn deserialize(&self, data: &[u8], world: &mut World) -> Result<(), DeserializationError>;
}