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

//! What the lane derives do with one output field: the [`OutputOps`] a field
//! attribute's check hands back, and the field types each output attribute
//! accepts.

use std::any::Any;

use crate::lane::guard::{OutputRefusal, Reason, SinkKind};
use crate::lane::io::{Declaration, OutputSink, Pass, PassSlot};
use crate::lane::reserve::Staged;
use crate::lane::LaneError;

/// What the derive does with one output field.
pub struct OutputOps<'f, T> {
    /// Declares the field's sink.
    pub declare: fn(&mut Declaration, &'static str),
    /// Opens the field for one record.
    pub open: fn(&mut OutputSink<'f>, &'static str) -> Result<T, LaneError>,
    /// Commits the field into the sink.
    pub commit: fn(T, &mut OutputSink<'f>, &'static str) -> Result<(), OutputRefusal>,
}

/// A field type `#[pass]` accepts: `Pass<'f, P>`.
#[diagnostic::on_unimplemented(
    message = "`#[pass]` records into a pass slot: the field must be `Pass<'f, P>`, not `{Self}`",
    label = "not a pass"
)]
pub trait PassField<'f>: Sized {
    /// Declares the pass slot written.
    fn declare(d: &mut Declaration, field: &'static str);
    /// Opens the pass for one record.
    fn open(sink: &mut OutputSink<'f>, field: &'static str) -> Result<Self, LaneError>;
    /// Closes the pass into its slot.
    fn commit(self, sink: &mut OutputSink<'f>, field: &'static str) -> Result<(), OutputRefusal>;
}
impl<'f, P: PassSlot> PassField<'f> for Pass<'f, P> {
    fn declare(d: &mut Declaration, field: &'static str) {
        d.write::<P>(field, SinkKind::Pass);
    }
    fn open(sink: &mut OutputSink<'f>, field: &'static str) -> Result<Self, LaneError> {
        let device = sink.device().ok_or_else(|| LaneError::MissingResource {
            kind: "pass encoder",
            key: format!("`{field}` ({}): no device lent", std::any::type_name::<P>()),
        })?;
        Ok(Pass::open(device).lending(sink.pipelines()))
    }
    fn commit(self, sink: &mut OutputSink<'f>, field: &'static str) -> Result<(), OutputRefusal> {
        let closed = self.close().ok_or(OutputRefusal {
            field,
            sink: SinkKind::Pass,
            reason: Reason::PassNotClosed,
        })?;
        sink.put_pass(field, closed)
    }
}

/// A field type output `#[deck]` accepts: an owned value a later phase reads.
#[diagnostic::on_unimplemented(
    message = "`#[deck]` writes a slot for a later phase: `{Self}` must be owned, `Default`, `Clone`, `Send` and `Sync`",
    label = "not an owned slot value"
)]
pub trait DeckOutputField<'f>: Sized {
    /// Declares the slot written.
    fn declare(d: &mut Declaration, field: &'static str);
    /// Opens the slot's value for one record: what it holds this frame, or
    /// its default.
    fn open(sink: &mut OutputSink<'f>, field: &'static str) -> Result<Self, LaneError>;
    /// Writes the value into its slot.
    fn commit(self, sink: &mut OutputSink<'f>, field: &'static str) -> Result<(), OutputRefusal>;
}
impl<'f, T: Default + Clone + Any + Send + Sync> DeckOutputField<'f> for T {
    fn declare(d: &mut Declaration, field: &'static str) {
        d.write::<T>(field, SinkKind::Deck);
    }
    fn open(sink: &mut OutputSink<'f>, _field: &'static str) -> Result<Self, LaneError> {
        Ok(sink.opening::<T>())
    }
    fn commit(self, sink: &mut OutputSink<'f>, field: &'static str) -> Result<(), OutputRefusal> {
        sink.put(field, self)
    }
}

/// A field type output `#[staged]` accepts: `Vec<T>` or [`Leases<T>`] of a
/// [`Staged`] output, kept in the family's reserve across frames.
#[diagnostic::on_unimplemented(
    message = "`#[staged]` writes to the family's reserve: the field must be `Vec<T>` or `Leases<T>` of a `Staged` output, not `{Self}`",
    label = "not staged outputs"
)]
pub trait StagedField<'f>: Sized {
    /// Declares the output staged.
    fn declare(d: &mut Declaration, field: &'static str);
    /// Opens the field for one record: nothing staged yet.
    fn open(sink: &mut OutputSink<'f>, field: &'static str) -> Result<Self, LaneError>;
    /// Stages what the lane wrote.
    fn commit(self, sink: &mut OutputSink<'f>, field: &'static str) -> Result<(), OutputRefusal>;
}
impl<'f, T: Staged> StagedField<'f> for Vec<T> {
    fn declare(d: &mut Declaration, field: &'static str) {
        d.write::<T>(field, SinkKind::Staged);
    }
    fn open(_sink: &mut OutputSink<'f>, _field: &'static str) -> Result<Self, LaneError> {
        Ok(Vec::new())
    }
    fn commit(self, sink: &mut OutputSink<'f>, field: &'static str) -> Result<(), OutputRefusal> {
        sink.stage(field, self)
    }
}

impl<'f, T: Staged> StagedField<'f> for crate::lane::Leases<T> {
    fn declare(d: &mut Declaration, field: &'static str) {
        d.write::<T>(field, SinkKind::Staged);
    }
    fn open(_sink: &mut OutputSink<'f>, _field: &'static str) -> Result<Self, LaneError> {
        Ok(Self::default())
    }
    fn commit(self, sink: &mut OutputSink<'f>, field: &'static str) -> Result<(), OutputRefusal> {
        for (items, lease) in self.into_groups() {
            match lease {
                Some(lease) => sink.stage_while(field, items, lease)?,
                None => sink.stage(field, items)?,
            }
        }
        Ok(())
    }
}

/// Checks a `#[pass]` field's type.
pub const fn pass_field<'f, T: PassField<'f>>() -> OutputOps<'f, T> {
    OutputOps {
        declare: <T as PassField<'f>>::declare,
        open: <T as PassField<'f>>::open,
        commit: <T as PassField<'f>>::commit,
    }
}
/// Checks an output `#[deck]` field's type.
pub const fn deck_output_field<'f, T: DeckOutputField<'f>>() -> OutputOps<'f, T> {
    OutputOps {
        declare: <T as DeckOutputField<'f>>::declare,
        open: <T as DeckOutputField<'f>>::open,
        commit: <T as DeckOutputField<'f>>::commit,
    }
}

/// Checks an output `#[staged]` field's type.
pub const fn staged_field<'f, T: StagedField<'f>>() -> OutputOps<'f, T> {
    OutputOps {
        declare: <T as StagedField<'f>>::declare,
        open: <T as StagedField<'f>>::open,
        commit: <T as StagedField<'f>>::commit,
    }
}
