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

//! What a lane's inputs and outputs are checked against before and after it
//! runs.
//!
//! Three layers, and none of them rewrites a value or panics:
//!
//! 1. every field: declared, present, from this frame, lock obtained in time;
//! 2. every source: the bus stamp, the target, the deck writer, the budget;
//! 3. every type that opts in with `#[bus(validate)]` / `#[deck(validate)]`,
//!    through [`Validate`].
//!
//! A refusal skips the lane for the frame; a warning lets it run.

use crate::problems::ProblemSeverity;

/// Where an input field is read from — one variant per `LaneInputs` field
/// attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SourceKind {
    /// `#[bus]`: a View a Flow published this frame.
    Bus,
    /// `#[frame]`: the target, the clear colour, the time.
    Frame,
    /// `#[deck]`: a slot an earlier phase wrote.
    Deck,
    /// `#[resident]`: what a residency made usable by a device.
    Resident,
    /// `#[resource]`: a runtime entry, read without locking.
    Resource,
    /// `#[locked]`: a runtime entry, locked.
    Locked,
    /// `#[budget]`: the frame's budget.
    Budget,
    /// `#[scope]`: the parallel scope.
    Scope,
}

/// Where an output field is written — one variant per `LaneOutputs` field
/// attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SinkKind {
    /// `#[pass]`: GPU commands for a pass slot.
    Pass,
    /// `#[deck]`: CPU data for another domain.
    Deck,
}

/// Why a field was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reason {
    /// The source does not hold it.
    Missing,
    /// It is from another frame.
    Stale,
    /// The lane did not declare it.
    Undeclared,
    /// Its lock was not obtained in time.
    LockTimedOut,
    /// Its lock was poisoned by a panic.
    LockPoisoned,
    /// The target has no size or a format the lane cannot draw to.
    BadTarget,
    /// The budget is not positive.
    NoBudget,
    /// A pass output was not closed.
    PassNotClosed,
    /// A deck output was written twice.
    WrittenTwice,
}

/// An input field the lane cannot run without, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InputRefusal {
    /// The field's name in the `LaneInputs` struct.
    pub field: &'static str,
    /// Where it is read from.
    pub source: SourceKind,
    /// Why it was refused.
    pub reason: Reason,
}

/// An output field that could not be committed, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OutputRefusal {
    /// The field's name in the `LaneOutputs` struct.
    pub field: &'static str,
    /// Where it is written.
    pub sink: SinkKind,
    /// Why it was refused.
    pub reason: Reason,
}

/// One problem a value's own check found in one of its fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueProblem {
    /// The dotted path of the field inside the value.
    pub field: &'static str,
    /// How much it matters.
    pub severity: ProblemSeverity,
    /// A sentence naming the value.
    pub message: String,
}

/// A value's own check, opted into with `#[bus(validate)]` or
/// `#[deck(validate)]`.
///
/// It names what is wrong and never rewrites it: a problem warns, the lane
/// still runs on the value as it is.
#[diagnostic::on_unimplemented(
    message = "`validate` checks `{Self}` with its `Validate` impl, and `{Self}` has none",
    label = "no `Validate` impl"
)]
pub trait Validate {
    /// Every problem in `self`; empty when nothing is wrong.
    fn validate(&self) -> Vec<ValueProblem>;
}

/// What a guard says about one field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing is wrong.
    Pass,
    /// Something looks wrong; the lane runs on the value as it is.
    Warn(Vec<ValueProblem>),
    /// The lane cannot run this frame.
    Refuse(InputRefusal),
}

/// A warning a fetch raised on one field, kept for `Problems`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputWarning {
    /// The field's name in the `LaneInputs` struct.
    pub field: &'static str,
    /// Where it was read from.
    pub source: SourceKind,
    /// What its check found.
    pub problems: Vec<ValueProblem>,
}
