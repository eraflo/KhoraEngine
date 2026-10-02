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

//! A number read at another type than it was written.

use serde::{Deserialize, Serialize};

use super::*;

/// A field whose type changed between the save and the code: a count that
/// became a float, a float that became a double.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Gauge {
    level: f32,
    precise: f64,
    count: u16,
}

/// A number whose type changed since the save is read when — and only when —
/// nothing is lost: the value read back is the value written, exactly. A
/// fraction, an out-of-range value or an integer a float cannot hold exactly
/// is an error, never a rounded or saturated guess.
#[test]
fn widening_is_lossless_or_refused() {
    // Integer to integer, in range.
    assert_eq!(read::<i64>(&Record::I64(-5)), Ok(-5));
    assert_eq!(read::<i32>(&Record::I64(-5)), Ok(-5));
    assert_eq!(read::<i8>(&Record::I64(-128)), Ok(-128));
    assert_eq!(read::<u8>(&Record::U64(255)), Ok(255));
    assert_eq!(read::<u32>(&Record::I64(7)), Ok(7));
    assert_eq!(read::<i64>(&Record::U64(7)), Ok(7));
    assert_eq!(read::<u64>(&Record::U64(u64::MAX)), Ok(u64::MAX));
    // ...and out of it.
    assert!(read::<u8>(&Record::U64(256)).is_err());
    assert!(read::<i8>(&Record::I64(-129)).is_err());
    assert!(read::<u32>(&Record::I64(-1)).is_err());
    assert!(read::<i64>(&Record::U64(u64::MAX)).is_err());
    assert!(read::<u16>(&Record::U64(u64::from(u16::MAX) + 1)).is_err());

    // Integer to float, when the float holds it exactly.
    assert_eq!(read::<f32>(&Record::I64(16_777_216)), Ok(16_777_216.0));
    assert_eq!(read::<f32>(&Record::I64(-3)), Ok(-3.0));
    assert_eq!(
        read::<f64>(&Record::I64(1 << 53)),
        Ok(9_007_199_254_740_992.0)
    );
    assert_eq!(read::<f64>(&Record::U64(1 << 60)), Ok(2.0_f64.powi(60)));
    assert!(read::<f32>(&Record::I64(16_777_217)).is_err());
    assert!(read::<f64>(&Record::I64((1 << 53) + 1)).is_err());
    // The largest integers round up to a power of two a cast back saturates
    // to them again: exactness has to be checked, not inferred from a cast.
    assert!(read::<f64>(&Record::I64(i64::MAX)).is_err());
    assert!(read::<f64>(&Record::U64(u64::MAX)).is_err());
    assert!(read::<f32>(&Record::U64(u64::MAX)).is_err());

    // Float to float.
    assert_eq!(
        read::<f64>(&Record::F32(0.1)).map(f64::to_bits),
        Ok(f64::from(0.1_f32).to_bits())
    );
    assert_eq!(read::<f64>(&Record::F32(-2.5)), Ok(-2.5));
    assert_eq!(read::<f32>(&Record::F64(0.5)), Ok(0.5));
    assert!(read::<f32>(&Record::F64(0.1)).is_err());
    assert!(read::<f32>(&Record::F64(1e300)).is_err());

    // Float to integer, when the float is a whole number in range.
    assert_eq!(read::<i64>(&Record::F64(3.0)), Ok(3));
    assert_eq!(read::<u32>(&Record::F32(2.0)), Ok(2));
    assert_eq!(read::<i16>(&Record::F64(-7.0)), Ok(-7));
    assert!(read::<i64>(&Record::F64(3.5)).is_err());
    assert!(read::<u32>(&Record::F32(0.25)).is_err());
    assert!(read::<u8>(&Record::F64(256.0)).is_err());
    assert!(read::<u8>(&Record::F64(-1.0)).is_err());
    assert!(read::<i64>(&Record::F64(9_223_372_036_854_775_808.0)).is_err());
    assert!(read::<i32>(&Record::F64(f64::NAN)).is_err());
    assert!(read::<i32>(&Record::F64(f64::INFINITY)).is_err());

    // Anything else is not a number that changed width.
    assert!(read::<i32>(&Record::Bool(true)).is_err());
    assert!(read::<bool>(&Record::I64(1)).is_err());
    assert!(read::<f32>(&text("1.0")).is_err());
    assert!(read::<String>(&Record::I64(1)).is_err());
    assert!(read::<u8>(&Record::Char('a')).is_err());

    // Inside a struct, the same rules, and a refused field fails the read.
    let widened = structure(
        "Gauge",
        vec![
            ("level", Record::I64(3)),
            ("precise", Record::F32(0.5)),
            ("count", Record::F64(12.0)),
        ],
    );
    assert_eq!(
        read::<Gauge>(&widened),
        Ok(Gauge {
            level: 3.0,
            precise: 0.5,
            count: 12,
        })
    );
    let lossy = structure(
        "Gauge",
        vec![
            ("level", Record::F32(1.0)),
            ("precise", Record::F64(0.5)),
            ("count", Record::F64(12.5)),
        ],
    );
    assert!(read::<Gauge>(&lossy).is_err());
}

/// A widened read is reported at the field it happened to, so an author
/// learns the save predates a type change; a number read at its own kind is
/// not a change.
#[test]
fn a_widened_number_is_reported() {
    let record = structure(
        "Gauge",
        vec![
            ("level", Record::I64(3)),
            ("precise", Record::F32(0.5)),
            ("count", Record::U64(12)),
        ],
    );
    let (value, mut entries) = resolve::<Gauge>(&record, &mut NoEntities).expect("resolves");
    assert_eq!(
        value,
        Gauge {
            level: 3.0,
            precise: 0.5,
            count: 12,
        }
    );
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    assert_eq!(
        entries,
        vec![
            ReportEntry {
                entity: None,
                component: None,
                path: "level".into(),
                kind: ReportKind::Widened,
            },
            ReportEntry {
                entity: None,
                component: None,
                path: "precise".into(),
                kind: ReportKind::Widened,
            },
        ]
    );
}
