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

//! Editor-specific custom agent registration.
//!
//! Built-in agents (Render, Shadow, Physics, UI, Audio) are registered
//! automatically by the engine. This module only registers editor extras.

use khora_sdk::DccService;

/// Registers editor-specific custom agents.
pub fn register_editor_agents(_dcc: &DccService) {
    // Built-in agents are already registered by the engine.
    // Add editor-only custom agents here (e.g. debug overlay, profiling).
    log::info!("Editor: Custom agent registration complete");
}
