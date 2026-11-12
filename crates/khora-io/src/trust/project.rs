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

//! The trust of the project open, shared in the runtime's resources.

use std::sync::{Arc, RwLock};

use khora_core::Runtime;

use super::ProjectTrust;

/// The trust of the project open, shared in `runtime.resources` as an
/// `Arc<ProjectTrustState>`.
#[derive(Debug)]
pub struct ProjectTrustState(RwLock<ProjectTrust>);

impl ProjectTrustState {
    /// A state holding `trust`.
    pub fn new(trust: ProjectTrust) -> Self {
        Self(RwLock::new(trust))
    }

    /// The trust held now.
    pub fn get(&self) -> ProjectTrust {
        match self.0.read() {
            Ok(trust) => *trust,
            Err(poisoned) => *poisoned.into_inner(),
        }
    }

    /// Holds `trust` from now on.
    pub fn set(&self, trust: ProjectTrust) {
        match self.0.write() {
            Ok(mut held) => *held = trust,
            Err(poisoned) => *poisoned.into_inner() = trust,
        }
    }

    /// The trust of the project `runtime` serves: its state's, `Trusted`
    /// when it holds none (a game, the sandbox, a test).
    pub fn of(runtime: &Runtime) -> ProjectTrust {
        runtime
            .resources
            .get::<Arc<ProjectTrustState>>()
            .map_or(ProjectTrust::Trusted, |state| state.get())
    }
}
