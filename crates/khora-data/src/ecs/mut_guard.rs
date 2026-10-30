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

//! `Mut`: writable access to one component's row, which notes the write.

use std::fmt;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

use crate::ecs::page::ComponentPage;
#[cfg(test)]
use crate::ecs::ColumnTicks;
use crate::ecs::{ComponentKey, Tick};

/// Writable access to one row of one component. Reading through it
/// (`Deref`) is not a change; the first write through it (`DerefMut`) stamps
/// the row with the run's tick and raises its column's summary.
///
/// Not `Send`: guards over rows of one column note their writes in the same
/// ticks, so two of them on two threads would race on them.
pub struct Mut<'w, T> {
    value: *mut T,
    ticks: Ticks,
    row: usize,
    tick: Tick,
    stamped: bool,
    _row: PhantomData<&'w mut T>,
}

/// Where a guard notes its write.
#[derive(Clone, Copy)]
enum Ticks {
    /// The column's ticks, reached directly.
    #[cfg(test)]
    Column(*mut ColumnTicks),
    /// The ticks of column `key` in `page`, looked up when the write is
    /// noted, so that guards over one column may live together: none of
    /// them keeps a pointer into the page's ticks while another may write
    /// them. An `observed` column without row ticks gains them then — in
    /// this page alone, and only once something is written.
    Page {
        page: *mut ComponentPage,
        key: ComponentKey,
        observed: bool,
        /// The change epoch of the column's domain, raised with the write.
        epoch: *mut u64,
    },
}

impl<'w, T> Mut<'w, T> {
    /// A guard over `value`, row `row` of the column whose ticks are `ticks`,
    /// stamping `tick` on its first write.
    ///
    /// # Safety
    ///
    /// `value` and `ticks` are valid for `'w`; no other reference to `*value`
    /// is live while the guard is; `ticks` is reached only through raw
    /// pointers while the guard lives; `row` is `value`'s row in that column.
    #[cfg(test)]
    pub(crate) unsafe fn new(
        value: *mut T,
        ticks: *mut ColumnTicks,
        row: usize,
        tick: Tick,
    ) -> Self {
        Self::with(value, Ticks::Column(ticks), row, tick)
    }

    /// A guard over `value`, row `row` of column `key` in `page`, stamping
    /// `tick` on its first write — giving the column row ticks first when it
    /// is `observed` and keeps none — and raising `epoch`, its domain's
    /// change epoch, to that tick. Guards made this way over one column may
    /// live together.
    ///
    /// # Safety
    ///
    /// `value`, `page` and `epoch` are valid for `'w`; no other reference to
    /// `*value` is live while the guard is; no reference to `page`'s ticks or
    /// entity list, nor to `epoch`, is live when the guard is written through;
    /// `row` is `value`'s row in column `key` of `page`.
    pub(crate) unsafe fn in_page(
        value: *mut T,
        page: *mut ComponentPage,
        key: ComponentKey,
        observed: bool,
        epoch: *mut u64,
        row: usize,
        tick: Tick,
    ) -> Self {
        Self::with(
            value,
            Ticks::Page {
                page,
                key,
                observed,
                epoch,
            },
            row,
            tick,
        )
    }

    fn with(value: *mut T, ticks: Ticks, row: usize, tick: Tick) -> Self {
        Self {
            value,
            ticks,
            row,
            tick,
            stamped: false,
            _row: PhantomData,
        }
    }

    /// Writes `value` only when it differs from the one held, stamping the
    /// row then; whether it wrote.
    pub fn set_if_neq(&mut self, value: T) -> bool
    where
        T: PartialEq,
    {
        if **self == value {
            return false;
        }
        **self = value;
        true
    }

    /// The value, writable, without stamping the row: a write through it is
    /// not seen as a change.
    pub fn bypass_change_detection(&mut self) -> &mut T {
        // SAFETY: `value` is valid for `'w` and no other reference to it is
        // live (see the constructors); the borrow is tied to `&mut self`.
        unsafe { &mut *self.value }
    }

    /// The value, writable for all of `'w`: the row is stamped now, as
    /// though written — whoever takes it may write it anywhere after.
    pub fn into_inner(mut self) -> &'w mut T {
        self.stamp();
        // SAFETY: as `bypass_change_detection`; the guard is consumed, so the
        // value is reached through this reference alone for `'w`.
        unsafe { &mut *self.value }
    }

    /// Notes the write, once.
    fn stamp(&mut self) {
        if self.stamped {
            return;
        }
        self.stamped = true;
        // SAFETY: the ticks are valid for `'w` and reached only through raw
        // pointers while the guard lives (see the constructors), so the
        // `&mut` made here is the only reference to them while it lives; it
        // ends before this returns.
        unsafe {
            match self.ticks {
                #[cfg(test)]
                Ticks::Column(ticks) => (*ticks).stamp(self.row, self.tick),
                Ticks::Page {
                    page,
                    key,
                    observed,
                    epoch,
                } => {
                    let rows = (*page).entities.len();
                    if let Some(ticks) = (*page).ticks.get_mut(&key) {
                        if observed {
                            ticks.materialize(rows);
                        }
                        ticks.stamp(self.row, self.tick);
                    }
                    *epoch = (*epoch).max(self.tick.0);
                }
            }
        }
    }
}

impl<T> Deref for Mut<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: `value` is valid for `'w` and no other reference to it is
        // live (see the constructors); the borrow is tied to `&self`.
        unsafe { &*self.value }
    }
}

impl<T> DerefMut for Mut<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.stamp();
        // SAFETY: as `bypass_change_detection`.
        unsafe { &mut *self.value }
    }
}

impl<T: fmt::Debug> fmt::Debug for Mut<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (**self).fmt(f)
    }
}
