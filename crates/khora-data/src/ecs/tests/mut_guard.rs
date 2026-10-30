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

//! `Mut`, the guard writable access hands out, on its own: one value and the
//! ticks of the column it is a row of.
//!
//! Reading through the guard is not a change. The first write through it
//! stamps its row τ with the guard's tick and raises the column's summary σ;
//! over a column that keeps no row ticks it raises σ and the floor φ, so the
//! row stays covered. `set_if_neq` writes — and stamps — only a value that
//! differs; `bypass_change_detection` writes without stamping.

use std::ptr;

use super::Position;
use crate::ecs::{ColumnTicks, Mut, Tick};

/// The ticks of a three-row column written at ticks 3, 2 and 3, its history
/// dropped at tick 1.
fn three_rows() -> ColumnTicks {
    ColumnTicks {
        summary: Tick(3),
        floor: Tick(1),
        rows: Some(vec![3, 2, 3]),
    }
}

/// The ticks of a column no observer watches: a summary, a floor following
/// it, no row ticks.
fn unwatched() -> ColumnTicks {
    ColumnTicks {
        summary: Tick(3),
        floor: Tick(3),
        rows: None,
    }
}

/// A guard over `value`, row `row` of the column `ticks`, at `tick`.
fn guard<'w>(
    value: &'w mut Position,
    ticks: &'w mut ColumnTicks,
    row: usize,
    tick: u64,
) -> Mut<'w, Position> {
    // SAFETY: both pointers come from exclusive borrows held for `'w`, which
    // the guard outlives no longer; nothing else reaches either while it does.
    unsafe { Mut::new(ptr::from_mut(value), ptr::from_mut(ticks), row, Tick(tick)) }
}

/// A guard only read leaves its column's ticks as they were.
#[test]
fn a_guard_only_read_leaves_its_column_ticks_alone() {
    let mut value = Position(4);
    let mut ticks = three_rows();
    {
        let read = guard(&mut value, &mut ticks, 1, 7);
        assert_eq!(read.0, 4, "the guard reads the value it guards");
        assert_eq!(*read, Position(4));
    }
    assert_eq!(ticks, three_rows(), "a read is not a change");
}

/// The first write through a guard stamps its row — that row alone — and
/// raises the column's summary to the guard's tick; the floor stays.
#[test]
fn the_first_write_through_a_guard_stamps_its_row_and_the_summary() {
    let mut value = Position(4);
    let mut ticks = three_rows();
    {
        let mut written = guard(&mut value, &mut ticks, 1, 7);
        written.0 = 9;
        written.0 += 1;
    }
    assert_eq!(value, Position(10), "the write reached the value");
    assert_eq!(
        ticks,
        ColumnTicks {
            summary: Tick(7),
            floor: Tick(1),
            rows: Some(vec![3, 7, 3]),
        }
    );
}

/// `set_if_neq` with the value already held writes nothing and stamps
/// nothing.
#[test]
fn set_if_neq_with_the_same_value_stamps_nothing() {
    let mut value = Position(4);
    let mut ticks = three_rows();
    {
        let mut same = guard(&mut value, &mut ticks, 1, 7);
        assert!(
            !same.set_if_neq(Position(4)),
            "an equal value is not written"
        );
    }
    assert_eq!(value, Position(4));
    assert_eq!(ticks, three_rows(), "an equal value is not a change");
}

/// `set_if_neq` with a value that differs writes it and stamps the row.
#[test]
fn set_if_neq_with_a_new_value_writes_and_stamps_the_row() {
    let mut value = Position(4);
    let mut ticks = three_rows();
    {
        let mut other = guard(&mut value, &mut ticks, 2, 7);
        assert!(other.set_if_neq(Position(5)), "a new value is written");
    }
    assert_eq!(value, Position(5));
    assert_eq!(
        ticks,
        ColumnTicks {
            summary: Tick(7),
            floor: Tick(1),
            rows: Some(vec![3, 2, 7]),
        }
    );
}

/// `bypass_change_detection` hands out the value writable and the write
/// through it is not seen.
#[test]
fn bypass_change_detection_writes_without_stamping() {
    let mut value = Position(4);
    let mut ticks = three_rows();
    {
        let mut quiet = guard(&mut value, &mut ticks, 0, 7);
        quiet.bypass_change_detection().0 = 11;
    }
    assert_eq!(value, Position(11), "the write reached the value");
    assert_eq!(ticks, three_rows(), "a bypassed write is not stamped");
}

/// Over a column that keeps no row ticks, the write raises the summary and
/// the floor with it — the row stays covered (I2) — and no row array
/// appears.
#[test]
fn a_guard_over_a_column_without_row_ticks_raises_its_summary_and_floor() {
    let mut value = Position(4);
    let mut ticks = unwatched();
    {
        let mut written = guard(&mut value, &mut ticks, 0, 7);
        written.0 = 9;
    }
    assert_eq!(value, Position(9));
    assert_eq!(
        ticks,
        ColumnTicks {
            summary: Tick(7),
            floor: Tick(7),
            rows: None,
        }
    );
}
