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

//! Why a layered agent is woken.

use crate::agent::gorna::StrategyId;
use crate::lane::guard::Reason;

/// Why a layered agent is woken. Nothing else calls it: on a frame without a
/// wake the scheduler applies its installed layer alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Wake {
    /// Nothing is installed at the family's place yet: its first frame.
    FirstFrame,
    /// An allocation was lent: a new envelope, pin or allowed rows.
    Allocated,
    /// The running row's quote no longer fits the envelope.
    QuoteUnfit,
    /// The running row's quote drifted past `QUOTE_DRIFT` from its value at
    /// the last full quote, or had none then.
    QuoteDrift,
    /// The last full quote is older than `QUOTE_MAX_AGE`.
    QuoteAge,
    /// The frame's target key changed: every quote was made on another.
    TargetChanged,
    /// The lane failed.
    LaneFailed,
    /// An input was refused, and why.
    InputRefused(Reason),
    /// The layer of this row, prepared off the frame, could not be
    /// prepared: the installed layer runs on.
    PrepareFailed(StrategyId),
}
