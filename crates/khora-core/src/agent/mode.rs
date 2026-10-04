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

//! Engine execution modes.
//!
//! The base engine only knows about **Playing** mode.
//! Plugins inject their own modes via `Custom(String)`.

/// The frame's engine mode, shared between the application that writes it and
/// the scheduler that reads it once per frame.
pub type SharedEngineMode = std::sync::Arc<std::sync::RwLock<EngineMode>>;

/// The current mode of the engine.
///
/// Different modes activate different agents and change rendering behavior.
/// The base engine only defines `Playing`; other modes are injected by plugins.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EngineMode {
    /// Simulation mode — scene cameras, physics, audio, ECS snapshot.
    Playing,
    /// A custom mode injected by a plugin.
    /// The string identifies the plugin's mode (e.g., `"editor"`).
    Custom(String),
}

impl EngineMode {
    /// Parses a mode from a string (case-insensitive).
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "playing" | "play" | "simulation" => Some(EngineMode::Playing),
            other => Some(EngineMode::Custom(other.to_string())),
        }
    }

    /// Returns a human-readable name for this mode.
    pub fn name(&self) -> &str {
        match self {
            EngineMode::Playing => "playing",
            EngineMode::Custom(s) => s,
        }
    }

    /// Whether gameplay — scripts, above all — runs in this mode.
    pub fn runs_gameplay(&self) -> bool {
        matches!(self, EngineMode::Playing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The game runs only while it is played: an editor's editing mode is not
    /// the game, and a tool mode is not either.
    #[test]
    fn only_playing_runs_gameplay() {
        assert!(EngineMode::Playing.runs_gameplay());
        assert!(!EngineMode::Custom("editor".to_owned()).runs_gameplay());
        assert!(!EngineMode::Custom("tool".to_owned()).runs_gameplay());
    }
}
