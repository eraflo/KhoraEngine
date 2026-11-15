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

//! A layer prepared off the frame, switching a family's row while its
//! installed layer keeps running. It goes through
//! [`Place::request_preparation`](super::Place::request_preparation) and
//! comes back through [`Place::prepared`](super::Place::prepared).

use std::time::{Duration, Instant};

use crate::agent::contention::Contention;
use crate::agent::gorna::StrategyId;
use crate::EngineContext;

use super::Layer;

/// A preparation's job: returns the prepared layer, or why it could not be prepared.
type Job = Box<dyn FnOnce(&mut EngineContext<'_>) -> Result<Box<dyn Layer>, String> + Send>;

/// A layer to prepare off the frame: its row, its contention and the job that prepares it.
pub struct Preparation {
    row: StrategyId,
    contention: Contention,
    job: Job,
}

impl std::fmt::Debug for Preparation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Preparation")
            .field("row", &self.row)
            .finish_non_exhaustive()
    }
}

impl Preparation {
    /// Returns the preparation of `row`'s layer by `job`, contending for `contention`.
    pub fn new(
        row: StrategyId,
        contention: Contention,
        job: impl FnOnce(&mut EngineContext<'_>) -> Result<Box<dyn Layer>, String> + Send + 'static,
    ) -> Self {
        Self {
            row,
            contention,
            job: Box::new(job),
        }
    }

    /// The row the preparation prepares.
    pub fn row(&self) -> StrategyId {
        self.row
    }

    /// What the preparation competes for: the permit its run is stamped
    /// with.
    pub fn contention(&self) -> Contention {
        self.contention.clone()
    }

    /// Runs the job on `context`, timing it: the layer's switch time.
    pub fn run(self, context: &mut EngineContext<'_>) -> PreparedLayer {
        let started = Instant::now();
        let outcome = (self.job)(context).map(|layer| (layer, started.elapsed()));
        PreparedLayer {
            row: self.row,
            outcome,
        }
    }
}

/// What a [`Preparation`] gave: the layer it prepared and the time it took,
/// or why it failed — for the place to take back.
pub struct PreparedLayer {
    row: StrategyId,
    outcome: Result<(Box<dyn Layer>, Duration), String>,
}

impl std::fmt::Debug for PreparedLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Prepared")
            .field("row", &self.row)
            .field("prepared", &self.outcome.is_ok())
            .finish()
    }
}

impl PreparedLayer {
    /// A preparation of `row` that gave no layer, for `message`: one that
    /// panicked, which its runner hands back in its place.
    pub fn failed(row: StrategyId, message: impl Into<String>) -> Self {
        Self {
            row,
            outcome: Err(message.into()),
        }
    }

    /// The row the preparation prepared.
    pub fn row(&self) -> StrategyId {
        self.row
    }

    /// The layer and the time it took to prepare, or why there is none.
    pub(in crate::agent::layer) fn into_outcome(
        self,
    ) -> Result<(Box<dyn Layer>, Duration), String> {
        self.outcome
    }
}

#[cfg(test)]
mod tests;
