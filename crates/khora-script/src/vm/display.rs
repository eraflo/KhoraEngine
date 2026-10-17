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

//! How a value is shown as text: what `$"…"` writes for each hole.
//!
//! One rule, chosen at compile time from the hole's type — at run time a
//! duration is a float, so its type is the only way to know it is one:
//!
//! | Type | Shown as |
//! |---|---|
//! | `int` | `42` |
//! | `float` | the shortest form that keeps its point: `2.0`, `0.1`, `1e-7`, `inf`, `NaN` |
//! | `Duration` | a float, then `s`: `2.5s` |
//! | `Angle` | in degrees to a thousandth, then `deg`: `90deg` |
//! | `bool`, `null` | `true`, `null` |
//! | `Vec2/3/4`, `Quat` | `(1.0, 2.0, 3.0)` |
//! | `Color` | `rgba(1.0, 0.5, 0.0, 1.0)` |
//! | `Entity` | `Entity#12`, `Entity#12v3` past generation 0 |
//! | `string` | as it is |

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use super::{Fault, Machine, Program, Reg, StrRef, Value};
use crate::native::Host;

/// How one hole of an interpolated string is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Shown {
    /// As its value says: a number by its kind, text as it is, the rest by
    /// the table.
    Value,
    /// As a float, an `int` held by a `float` too.
    Float,
    /// Seconds, then `s`.
    Duration,
    /// Radians, shown in degrees, then `deg`.
    Angle,
}

/// One part of an interpolated string's plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FormatPiece {
    /// Text written as it is.
    Text(String),
    /// The next hole's value, shown so.
    Hole(Shown),
}

/// What an interpolated string writes: its text and how each hole is shown,
/// in order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FormatPlan {
    /// Its parts, in order.
    pub pieces: Vec<FormatPiece>,
}

impl Machine {
    /// `$"…"`: `count` values from `base` shown by `plan`, joined into one
    /// new text — one allocation, whatever the holes.
    pub(super) fn step_format(
        &mut self,
        program: &Program,
        host: &mut Host,
        dst: Reg,
        base: Reg,
        plan: u32,
    ) -> Result<(), Fault> {
        let plan = program.formats.get(plan as usize).ok_or(Fault::BadString)?;
        let mut text = String::new();
        let mut holes = 0u16;
        for piece in &plan.pieces {
            match piece {
                FormatPiece::Text(written) => text.push_str(written),
                FormatPiece::Hole(shown) => {
                    let index = Reg::try_from(u16::from(base) + holes)
                        .map_err(|_| Fault::BadRegister { index: Reg::MAX })?;
                    holes += 1;
                    let value = self.read(index)?;
                    match value {
                        Value::Str(_) => text.push_str(self.resolve_str(value, program, host)?),
                        other => show(other, *shown, &mut text),
                    }
                }
            }
        }
        let reference = host
            .arena
            .alloc(crate::arena::Object::Str(text))
            .map_err(|_| Fault::ArenaFull)?;
        self.write(dst, Value::Str(StrRef::Arena(reference)))
    }
}

/// Writes `value`, which is not text, as `shown` says.
pub(crate) fn show(value: Value, shown: Shown, out: &mut String) {
    let number = match value {
        Value::Int(i) if shown == Shown::Value => {
            let _ = write!(out, "{i}");
            return;
        }
        Value::Int(i) => Some(i as f32),
        Value::Float(f) => Some(f),
        _ => None,
    };
    if let Some(number) = number {
        match shown {
            Shown::Value | Shown::Float => float(number, out),
            Shown::Duration => {
                float(number, out);
                out.push('s');
            }
            Shown::Angle => {
                let degrees = (f64::from(number).to_degrees() * 1000.0).round() / 1000.0;
                // `-0` is shown as the zero it is.
                let degrees = if degrees == 0.0 { 0.0 } else { degrees };
                let _ = write!(out, "{degrees}deg");
            }
        }
        return;
    }
    match value {
        Value::Bool(b) => {
            let _ = write!(out, "{b}");
        }
        Value::Null => out.push_str("null"),
        Value::Vec2(v) => components(&[v.x, v.y], out),
        Value::Vec3(v) => components(&[v.x, v.y, v.z], out),
        Value::Vec4(v) => components(&[v.x, v.y, v.z, v.w], out),
        Value::Quat(q) => components(&[q.x, q.y, q.z, q.w], out),
        Value::Color(c) => {
            out.push_str("rgba");
            components(&[c.r, c.g, c.b, c.a], out);
        }
        Value::Entity(entity) if entity.generation == 0 => {
            let _ = write!(out, "Entity#{}", entity.index);
        }
        Value::Entity(entity) => {
            let _ = write!(out, "Entity#{}v{}", entity.index, entity.generation);
        }
        // Nothing the checker lets into a hole.
        _ => out.push('?'),
    }
}

/// A float in its shortest form that keeps its point: `2.0`, `0.1`, `1e-7`.
fn float(value: f32, out: &mut String) {
    let _ = write!(out, "{value:?}");
}

/// `(1.0, 2.0, 3.0)`.
fn components(values: &[f32], out: &mut String) {
    out.push('(');
    for (at, value) in values.iter().enumerate() {
        if at > 0 {
            out.push_str(", ");
        }
        float(*value, out);
    }
    out.push(')');
}
