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

//! What the heuristic engine concludes from one analysis pass.

/// Analysis results and alerts produced by the `HeuristicEngine`.
#[derive(Debug, Clone)]
pub struct AnalysisReport {
    /// `true` if a resource conflict or performance drop is detected and GORNA
    /// should run a full negotiation round.
    pub needs_negotiation: bool,
    /// Suggested global target latency (in ms) derived from analysis.
    pub suggested_latency_ms: f32,
    /// `true` if the engine is in a "death spiral" — multiple subsystems are
    /// simultaneously failing to meet budgets and an emergency stop is required.
    pub death_spiral_detected: bool,
    /// Human-readable summary of analysis findings for telemetry/logging.
    pub alerts: Vec<String>,
}

impl Default for AnalysisReport {
    fn default() -> Self {
        Self {
            needs_negotiation: false,
            suggested_latency_ms: 16.66,
            death_spiral_detected: false,
            alerts: Vec::new(),
        }
    }
}
