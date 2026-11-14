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

//! A family's outputs, as `#[derive(LaneOutputs)]` declares them.

use super::{Declaration, OutputSink};
use crate::lane::guard::OutputRefusal;
use crate::lane::LaneError;

/// A family's outputs, derived with `#[derive(LaneOutputs)]`.
pub trait LaneOutputs<'f>: Sized {
    /// Declares every field's sink.
    fn declare(d: &mut Declaration);

    /// Opens every field for one record.
    fn open(sink: &mut OutputSink<'f>) -> Result<Self, LaneError>;

    /// Commits every field into `sink`, running the output guards.
    fn commit(self, sink: &mut OutputSink<'f>) -> Result<(), OutputRefusal>;
}
