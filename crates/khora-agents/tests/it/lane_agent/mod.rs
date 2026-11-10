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

//! `LaneAgent`, the one agent behaviour, and the registrations it is built
//! from.

mod background;
mod cheaper_lane_runs;
mod choice;
mod choice_across_lanes;
mod default_level;
mod envelope;
mod execution;
mod fixtures;
mod frames_without_a_scheduler;
mod health;
mod impact_change;
mod lane_message;
mod layer_preparations;
mod layers;
mod layers_reinstated;
mod layers_scheduled;
mod lever_across_lanes;
mod menu;
mod offer;
mod offer_during_a_swap;
mod pass_pipelines;
mod quote_impact;
mod quote_share;
mod quotes;
mod registration;
mod scheduled;
