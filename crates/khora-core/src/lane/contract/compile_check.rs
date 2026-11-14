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

//! The compile-time checks the lane derives and `register_agent!` emit, and
//! the per-field work they hand back.
//!
//! Each field attribute of `#[derive(LaneInputs)]` and `#[derive(LaneOutputs)]`
//! accepts one shape of field type. The derive calls the attribute's check
//! with the field's type, spanned on that type, and the check returns that
//! field's operations: how to declare it, how to fetch or open it. Declaring
//! and fetching go through the same check, so a field of the wrong shape fails
//! once, on the field, with the attribute's own message — never somewhere
//! inside the generated `fetch`. Not for hand-written code.

use std::any::Any;
use std::sync::{Arc, Mutex};

use crate::event::Supersedes;
use crate::lane::context_keys::ClearColor;
use crate::lane::contract::Lane;
use crate::lane::family::LaneFamily;
use crate::lane::guard::{InputRefusal, SourceKind};
use crate::lane::io::{
    Budget, Declaration, FixedStep, InputSource, Locked, SceneTarget, Target, TargetKey,
};
use crate::residency::{Residency, Resident};

pub use super::output_ops::{
    deck_output_field, pass_field, staged_field, DeckOutputField, OutputOps, PassField, StagedField,
};

/// What the derive does with one input field.
pub struct InputOps<'f, T> {
    /// Declares the field's source.
    pub declare: fn(&mut Declaration, &'static str),
    /// Fetches the field.
    pub fetch: fn(&mut InputSource<'f>, &'static str) -> Result<T, InputRefusal>,
    /// The target the field names, for a `#[frame] Target`.
    pub target: fn(&T) -> Option<TargetKey>,
}

/// A field that names no target.
fn no_target<T>(_field: &T) -> Option<TargetKey> {
    None
}

/// A field type `#[bus]` accepts: `&'f V`, a View.
#[diagnostic::on_unimplemented(
    message = "`#[bus]` reads a View: the field must be `&'f V`, not `{Self}`",
    label = "not a View reference"
)]
pub trait BusField<'f>: Sized {
    /// Declares the View read.
    fn declare(d: &mut Declaration, field: &'static str);
    /// Fetches the View published for this frame.
    fn fetch(src: &mut InputSource<'f>, field: &'static str) -> Result<Self, InputRefusal>;
}
impl<'f, V: Any + Send + Sync> BusField<'f> for &'f V {
    fn declare(d: &mut Declaration, field: &'static str) {
        d.read::<V>(field, SourceKind::Bus);
    }
    fn fetch(src: &mut InputSource<'f>, field: &'static str) -> Result<Self, InputRefusal> {
        src.view::<V>(field)
    }
}

/// A field type input `#[deck]` accepts: `Option<&'f T>`, a slot an earlier
/// phase may have written.
#[diagnostic::on_unimplemented(
    message = "`#[deck]` reads a slot an earlier phase may not have written: the field must be `Option<&'f T>`, not `{Self}`",
    label = "not an optional slot reference"
)]
pub trait DeckInputField<'f>: Sized {
    /// Declares the slot read.
    fn declare(d: &mut Declaration, field: &'static str);
    /// Fetches the slot, if an earlier phase wrote it.
    fn fetch(src: &mut InputSource<'f>, field: &'static str) -> Result<Self, InputRefusal>;
}
impl<'f, T: Any + Send + Sync> DeckInputField<'f> for Option<&'f T> {
    fn declare(d: &mut Declaration, field: &'static str) {
        d.read::<T>(field, SourceKind::Deck);
    }
    fn fetch(src: &mut InputSource<'f>, field: &'static str) -> Result<Self, InputRefusal> {
        Ok(src.deck_slot::<T>(field))
    }
}

/// A field type `#[resident]` accepts: `&'f Resident<R>`.
#[diagnostic::on_unimplemented(
    message = "`#[resident]` reads what a residency made usable: the field must be `&'f Resident<R>`, not `{Self}`",
    label = "not a resident reference"
)]
pub trait ResidentField<'f>: Sized {
    /// Declares the residency's store read.
    fn declare(d: &mut Declaration, field: &'static str);
    /// Fetches the residency's store.
    fn fetch(src: &mut InputSource<'f>, field: &'static str) -> Result<Self, InputRefusal>;
}
impl<'f, R: Residency> ResidentField<'f> for &'f Resident<R> {
    fn declare(d: &mut Declaration, field: &'static str) {
        d.read::<Resident<R>>(field, SourceKind::Resident);
    }
    fn fetch(src: &mut InputSource<'f>, field: &'static str) -> Result<Self, InputRefusal> {
        src.entry::<Resident<R>>(field, SourceKind::Resident)
    }
}

/// A field type `#[resource]` accepts: `&'f T`, read without locking.
#[diagnostic::on_unimplemented(
    message = "`#[resource]` reads a runtime entry: the field must be `&'f T`, not `{Self}`",
    label = "not a reference"
)]
pub trait ResourceField<'f>: Sized {
    /// Declares the runtime entry read.
    fn declare(d: &mut Declaration, field: &'static str);
    /// Fetches the runtime entry.
    fn fetch(src: &mut InputSource<'f>, field: &'static str) -> Result<Self, InputRefusal>;
}
impl<'f, T: Any + Send + Sync> ResourceField<'f> for &'f T {
    fn declare(d: &mut Declaration, field: &'static str) {
        d.read::<T>(field, SourceKind::Resource);
    }
    fn fetch(src: &mut InputSource<'f>, field: &'static str) -> Result<Self, InputRefusal> {
        src.entry::<T>(field, SourceKind::Resource)
    }
}

/// A field type `#[locked]` accepts: `Locked<'f, T>`.
#[diagnostic::on_unimplemented(
    message = "`#[locked]` takes a lock: the field must be `Locked<'f, T>`, not `{Self}`",
    label = "not a lock",
    note = "a plain reference is `#[resource]`: a read that takes no lock"
)]
pub trait LockedField<'f>: Sized {
    /// Declares the lock of the registered `Arc<Mutex<T>>`.
    fn declare(d: &mut Declaration, field: &'static str);
    /// Takes the lock.
    fn fetch(src: &mut InputSource<'f>, field: &'static str) -> Result<Self, InputRefusal>;
}
impl<'f, T: ?Sized + Send + 'static> LockedField<'f> for Locked<'f, T> {
    fn declare(d: &mut Declaration, field: &'static str) {
        d.read::<Arc<Mutex<T>>>(field, SourceKind::Locked);
    }
    fn fetch(src: &mut InputSource<'f>, field: &'static str) -> Result<Self, InputRefusal> {
        src.lock::<T>(field)
    }
}

/// A field type `#[budget]` accepts: `Budget`.
#[diagnostic::on_unimplemented(
    message = "`#[budget]` gives the frame's budget: the field must be `Budget`, not `{Self}`",
    label = "not a `Budget`"
)]
pub trait BudgetField: Sized {
    /// Declares the budget read.
    fn declare(d: &mut Declaration, field: &'static str);
    /// Fetches the budget.
    fn fetch(src: &mut InputSource<'_>, field: &'static str) -> Result<Self, InputRefusal>;
}
impl BudgetField for Budget {
    fn declare(d: &mut Declaration, field: &'static str) {
        d.read::<Budget>(field, SourceKind::Budget);
    }
    fn fetch(src: &mut InputSource<'_>, field: &'static str) -> Result<Self, InputRefusal> {
        src.budget(field)
    }
}
/// A lane that can run before its agent was lent anything declares
/// `Option<Budget>`: `None` until a budget is lent — a budget of zero is one,
/// and buys nothing — and the lane decides what it runs on meanwhile: its
/// level's own price on the frame's inputs, say.
impl BudgetField for Option<Budget> {
    fn declare(d: &mut Declaration, field: &'static str) {
        d.read::<Budget>(field, SourceKind::Budget);
    }
    fn fetch(src: &mut InputSource<'_>, field: &'static str) -> Result<Self, InputRefusal> {
        Ok(src.lent_budget(field))
    }
}

/// A field type `#[take]` accepts: `Vec<T>`, the requests the family's
/// `Take` mailbox on the runtime's `Channel<T>` holds, drained.
#[diagnostic::on_unimplemented(
    message = "`#[take]` drains the family's mailbox: the field must be `Vec<T>` of a `Supersedes` item, not `{Self}`",
    label = "not a `Vec` of requests"
)]
pub trait TakeField<'f>: Sized {
    /// Declares the channel taken from.
    fn declare(d: &mut Declaration, field: &'static str);
    /// Drains the channel.
    fn fetch(src: &mut InputSource<'f>, field: &'static str) -> Result<Self, InputRefusal>;
}
impl<'f, T: Supersedes + Clone + Send + Sync + 'static> TakeField<'f> for Vec<T> {
    fn declare(d: &mut Declaration, field: &'static str) {
        d.read::<crate::event::Channel<T>>(field, SourceKind::Take);
    }
    fn fetch(src: &mut InputSource<'f>, field: &'static str) -> Result<Self, InputRefusal> {
        src.take(field)
    }
}

/// A field type `#[frame]` accepts: the frame's `Target` or `ClearColor`.
#[diagnostic::on_unimplemented(
    message = "`#[frame]` gives the frame's `Target`, `ClearColor` or `FixedStep`, not `{Self}`",
    label = "not a frame value"
)]
pub trait FrameField: Sized {
    /// Declares the frame value read.
    fn declare(d: &mut Declaration, field: &'static str);
    /// Fetches the frame value.
    fn fetch(src: &mut InputSource<'_>, field: &'static str) -> Result<Self, InputRefusal>;
    /// The target the value names, if it is one.
    fn target_key(&self) -> Option<TargetKey>;
}
impl FrameField for Target {
    fn declare(d: &mut Declaration, field: &'static str) {
        d.read::<Target>(field, SourceKind::Frame);
    }
    fn fetch(src: &mut InputSource<'_>, field: &'static str) -> Result<Self, InputRefusal> {
        src.target(field)
    }
    fn target_key(&self) -> Option<TargetKey> {
        Some(self.key)
    }
}
/// Read as the frame's [`Target`]: the same source, refused without depth.
impl FrameField for SceneTarget {
    fn declare(d: &mut Declaration, field: &'static str) {
        d.read::<Target>(field, SourceKind::Frame);
    }
    fn fetch(src: &mut InputSource<'_>, field: &'static str) -> Result<Self, InputRefusal> {
        src.scene_target(field)
    }
    fn target_key(&self) -> Option<TargetKey> {
        Some(self.key)
    }
}
impl FrameField for FixedStep {
    fn declare(d: &mut Declaration, field: &'static str) {
        d.read::<FixedStep>(field, SourceKind::Frame);
    }
    fn fetch(src: &mut InputSource<'_>, field: &'static str) -> Result<Self, InputRefusal> {
        src.fixed_step(field)
    }
    fn target_key(&self) -> Option<TargetKey> {
        None
    }
}
impl FrameField for ClearColor {
    fn declare(d: &mut Declaration, field: &'static str) {
        d.read::<ClearColor>(field, SourceKind::Frame);
    }
    fn fetch(src: &mut InputSource<'_>, field: &'static str) -> Result<Self, InputRefusal> {
        src.clear_color(field)
    }
    fn target_key(&self) -> Option<TargetKey> {
        None
    }
}

/// Checks a `#[bus]` field's type.
pub const fn bus_field<'f, T: BusField<'f>>() -> InputOps<'f, T> {
    InputOps {
        declare: <T as BusField<'f>>::declare,
        fetch: <T as BusField<'f>>::fetch,
        target: no_target::<T>,
    }
}
/// Checks an input `#[deck]` field's type.
pub const fn deck_input_field<'f, T: DeckInputField<'f>>() -> InputOps<'f, T> {
    InputOps {
        declare: <T as DeckInputField<'f>>::declare,
        fetch: <T as DeckInputField<'f>>::fetch,
        target: no_target::<T>,
    }
}
/// Checks a `#[resident]` field's type.
pub const fn resident_field<'f, T: ResidentField<'f>>() -> InputOps<'f, T> {
    InputOps {
        declare: <T as ResidentField<'f>>::declare,
        fetch: <T as ResidentField<'f>>::fetch,
        target: no_target::<T>,
    }
}
/// Checks a `#[resource]` field's type.
pub const fn resource_field<'f, T: ResourceField<'f>>() -> InputOps<'f, T> {
    InputOps {
        declare: <T as ResourceField<'f>>::declare,
        fetch: <T as ResourceField<'f>>::fetch,
        target: no_target::<T>,
    }
}
/// Checks a `#[locked]` field's type.
pub const fn locked_field<'f, T: LockedField<'f>>() -> InputOps<'f, T> {
    InputOps {
        declare: <T as LockedField<'f>>::declare,
        fetch: <T as LockedField<'f>>::fetch,
        target: no_target::<T>,
    }
}
/// Checks a `#[budget]` field's type.
pub const fn budget_field<'f, T: BudgetField>() -> InputOps<'f, T> {
    InputOps {
        declare: <T as BudgetField>::declare,
        fetch: <T as BudgetField>::fetch,
        target: no_target::<T>,
    }
}
/// Checks a `#[take]` field's type.
pub const fn take_field<'f, T: TakeField<'f>>() -> InputOps<'f, T> {
    InputOps {
        declare: <T as TakeField<'f>>::declare,
        fetch: <T as TakeField<'f>>::fetch,
        target: no_target::<T>,
    }
}
/// Checks a `#[frame]` field's type.
pub const fn frame_field<'f, T: FrameField>() -> InputOps<'f, T> {
    InputOps {
        declare: <T as FrameField>::declare,
        fetch: <T as FrameField>::fetch,
        target: <T as FrameField>::target_key,
    }
}
/// Checks that lane `L` writes family `F`'s outputs: every lane of one agent
/// shares its family's outputs.
pub const fn lane_of_family<F: LaneFamily, L: Lane<Family = F>>() {}
