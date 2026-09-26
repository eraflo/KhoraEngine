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

//! The agent that negotiates for gameplay.
//!
//! Scripting has an agent for one reason: the DCC has to be able to say "you
//! have 0.4ms, hand back control", and a language that cannot be told that
//! makes the frame budget a suggestion. Every other subsystem here already
//! degrades under pressure; before Ergon, gameplay was the one that could not.
//!
//! # What the agent does, and what it does not
//!
//! It picks a lane to fit the budget and hands that lane what it needs: the
//! view, the fuel, the locked runtime, the deck, the queues the engine fills.
//! Then it dispatches. The lane applies reloads, delivers mail, runs behaviors
//! and records what that cost — because that is *work*, and `RULES.md` §8 says
//! an agent is a strategist.
//!
//! It used to do all of it, and own the scripting runtime besides. What the
//! shape cost was not correctness but reach: nothing else could see the live
//! behaviors, because they were a private field of a strategist.
//!
//! # The budget is a time, and fuel is how it is spent
//!
//! [`apply_budget`](khora_core::agent::Agent::apply_budget) receives a `Duration` and converts it
//! at a rate the **lane** measures rather than one assumed here. A fixed
//! instructions-per-millisecond constant would be wrong on the first machine
//! that was not the one it was written on; the rate starts at an estimate and
//! is corrected by what the last frames actually cost.
//!
//! # Why it can run in parallel
//!
//! [`access`](khora_core::agent::Agent::access) is [`AgentAccess::Isolated`](khora_core::agent::AgentAccess::Isolated), which is a claim
//! about the `World` alone: no lane here reads or writes one, because a
//! script's effects are queued as commands rather than applied. What it *does*
//! reach — the runtime, the two channels, two deck slots — is declared in
//! [`contention`](khora_core::agent::Agent::contention), and that declaration is what keeps a
//! concurrent agent out of the same state.

mod agent;

pub use agent::*;
