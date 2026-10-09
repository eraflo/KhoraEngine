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

//! Resuming a frozen body in the code as it is now.
//!
//! One path for every resume — a frame boundary, a hot reload, a load — that
//! degrades in tiers instead of all-or-nothing.

use khora_core::script::{FrozenFrame, FrozenLocal, FrozenMachine, FrozenValue};

use super::freeze::{thaw_arguments, thaw_held};
use super::site::{Site, SiteKind};
use super::{Frame, Machine, Program, Value};

/// How a frozen body came back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeTier {
    /// The program is the one it was frozen in.
    Exact,
    /// Every function on its stack is unchanged.
    Unchanged,
    /// Its frames were rebuilt at the same sites in edited code.
    Rebuilt,
    /// Its member runs again from its entry, with its original arguments.
    Restarted,
}

/// A frozen body nothing in the program can take back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Abandoned {
    /// The abandoned body's member path: its outermost function's name without
    /// the behavior — `"OnSpawn"`, `"Patrol.OnHit"`, `"__every(5)"`.
    pub member: String,
}

/// `frozen`, frozen in a program whose fingerprint was `fingerprint`, as a
/// machine of `program`.
///
/// Tried in order, each tier only when the one before cannot hold it:
/// 1. **exact** — the same program;
/// 2. **unchanged** — every function on the stack has the fingerprint it was
///    frozen with, so every position in it still means what it meant;
/// 3. **rebuilt** — every frame's site still exists, its locals by name and
///    type, its temporaries as many: the frames are laid out again at the new
///    sites;
/// 4. **restarted** — the body's member still takes the same parameters: it
///    runs again from its entry with the arguments it was first given. What it
///    did before it was cut may happen again; the author edited the code it was
///    running, and the alternative is losing the body;
/// 5. otherwise [`Abandoned`].
pub fn resume(
    frozen: &FrozenMachine,
    fingerprint: u64,
    program: &Program,
) -> Result<(Machine, ResumeTier), Abandoned> {
    if fingerprint == program.fingerprint() {
        if let Some(machine) = Machine::thaw(frozen, program) {
            return Ok((machine, ResumeTier::Exact));
        }
    }
    if !frozen.frames.is_empty() && frozen.frames.iter().all(|frame| unchanged(frame, program)) {
        if let Some(machine) = Machine::thaw(frozen, program) {
            return Ok((machine, ResumeTier::Unchanged));
        }
    }
    if let Some(machine) = rebuild(frozen, program) {
        return Ok((machine, ResumeTier::Rebuilt));
    }
    if let Some(machine) = restart(frozen, program) {
        return Ok((machine, ResumeTier::Restarted));
    }
    Err(Abandoned {
        member: frozen
            .frames
            .first()
            .map(|frame| member_of(&frame.function).to_owned())
            .unwrap_or_default(),
    })
}

/// A function name without the behavior it belongs to.
fn member_of(function: &str) -> &str {
    function
        .split_once('.')
        .map_or(function, |(_, member)| member)
}

/// Whether `frame`'s function is, in `program`, the code it was frozen in.
fn unchanged(frame: &FrozenFrame, program: &Program) -> bool {
    frame.fingerprint != 0
        && program
            .function(&frame.function)
            .is_some_and(|function| function.fingerprint == frame.fingerprint)
}

/// Where a frame's caller leaves it: the caller's base, where the frame's
/// window begins and its result lands relative to it, and the instruction the
/// caller resumes at.
struct Call {
    caller_base: usize,
    base: usize,
    result: usize,
    return_pc: usize,
}

/// The machine with every edited frame laid out again at its site in
/// `program`, and every unchanged frame kept as it was frozen.
///
/// A frame whose function kept its fingerprint is the same code: its window,
/// its counter and its call into the next frame mean what they meant, and its
/// sites may still be named differently — a renamed local changes no code but
/// renames the statement that declares it. Only a frame whose code changed is
/// moved to its site by name.
fn rebuild(frozen: &FrozenMachine, program: &Program) -> Option<Machine> {
    let last = frozen.frames.len().checked_sub(1)?;
    // A save is input nobody sized: no deeper than a running machine can be.
    if frozen.frames.len() > super::MAX_FRAMES {
        return None;
    }
    let mut registers: Vec<Value> = Vec::new();
    let mut frames: Vec<Frame> = Vec::new();
    let mut call: Option<Call> = None;
    let mut program_counter = 0;
    let mut held: Vec<crate::arena::Owned> = Vec::new();

    for (depth, old) in frozen.frames.iter().enumerate() {
        let index = program.index_of(&old.function)?;
        let function = program.functions.get(index)?;
        let (base, result, return_pc) = match &call {
            None => (0, 0, 0),
            Some(call) => (
                call.caller_base.checked_add(call.base)?,
                call.caller_base.checked_add(call.result)?,
                call.return_pc,
            ),
        };
        let end = base.checked_add(function.registers)?;
        if registers.len() < end {
            registers.resize(end, Value::Unit);
        }
        // The window belongs to this frame from its start: a caller's argument
        // slots there are this frame's parameters, written below.
        for register in &mut registers[base..end] {
            *register = Value::Unit;
        }

        let old_base = usize::try_from(old.base).ok()?;
        let read = |register: usize, held: &mut Vec<crate::arena::Owned>| -> Option<Value> {
            thaw_held(
                frozen.registers.get(old_base.checked_add(register)?)?,
                program,
                held,
            )
        };
        let callee = frozen.frames.get(depth + 1);

        if unchanged(old, program) {
            // As frozen: the whole window, the counter, the call it made.
            for register in 0..function.registers {
                if let Some(value) = frozen.registers.get(old_base.checked_add(register)?) {
                    registers[base + register] = thaw_held(value, program, &mut held)?;
                }
            }
            program_counter = usize::try_from(frozen.program_counter).ok()?;
            call = match callee {
                Some(callee) => {
                    let at = usize::try_from(callee.base).ok()?.checked_sub(old_base)?;
                    let result = usize::try_from(callee.result).ok()?.checked_sub(old_base)?;
                    // Both name registers of this frame's window, as a call
                    // the compiler emitted does.
                    if at >= function.registers || result >= function.registers {
                        return None;
                    }
                    Some(Call {
                        caller_base: base,
                        base: at,
                        result,
                        return_pc: usize::try_from(callee.return_pc).ok()?,
                    })
                }
                None => None,
            };
        } else {
            if old.site.is_empty() {
                return None;
            }
            let site = function.sites.iter().find(|site| site.name == old.site)?;
            // Only the innermost frame stands anywhere but at a call's return.
            if depth < last && !matches!(site.kind, SiteKind::Return { .. }) {
                return None;
            }
            for (now, was, ty) in matched_locals(site, &old.locals)? {
                let was = usize::try_from(was).ok()?;
                // A local keeps its name and its type's name and still cannot
                // take the value: a `var` that held an optional's `null`, now
                // an `int`. Its frame is not rebuilt; the body restarts.
                if !holds(
                    frozen.registers.get(old_base.checked_add(was)?)?,
                    ty,
                    &program.structs,
                ) {
                    return None;
                }
                *registers.get_mut(base + usize::from(now))? = read(was, &mut held)?;
            }
            if site.temporaries.len() != old.temporaries.len() {
                return None;
            }
            // A frame still waiting on its call has no result yet: the last
            // temporary of a return site is where the result will land, and
            // what the frozen register held there was the callee's.
            let waiting = usize::from(callee.is_some());
            let kept = site.temporaries.len().saturating_sub(waiting);
            for (&now, &was) in site.temporaries.iter().zip(&old.temporaries).take(kept) {
                *registers.get_mut(base + usize::from(now))? =
                    read(usize::try_from(was).ok()?, &mut held)?;
            }
            program_counter = site.pc as usize;
            call = match (callee, site.kind) {
                (Some(_), SiteKind::Return { base: at, dst }) => Some(Call {
                    caller_base: base,
                    base: usize::from(at),
                    result: usize::from(dst),
                    return_pc: site.pc as usize,
                }),
                (Some(_), _) => return None,
                (None, _) => None,
            };
        }

        frames.push(Frame {
            function: index,
            base,
            return_pc,
            result,
        });
    }

    // Every frame unchanged is tier 2's case, which a thaw already refused.
    if frozen.frames.iter().all(|frame| unchanged(frame, program)) {
        return None;
    }
    let arguments = thaw_arguments(frozen, program, &mut held).unwrap_or_default();
    // What the frames hold must come back into the edited program — a frame
    // kept as frozen is not checked local by local.
    if !super::freeze::holds_all(&held, program) {
        return None;
    }
    Some(Machine {
        registers,
        frames,
        program_counter,
        finished: false,
        arguments,
        held,
        origins: Default::default(),
    })
}

/// Each local the new site holds, paired with the register it had where the
/// frame was frozen: the local of the same name, declared in the same block,
/// of the same type. A block is named by its enclosing statements' headers,
/// so a local keeps its pairing when its own initialiser is edited, and a
/// local and one it shadows never take each other's value whichever of the
/// two an edit removed. `None` when the new site needs a local the old one did
/// not have.
fn matched_locals<'s>(site: &'s Site, old: &[FrozenLocal]) -> Option<Vec<(u8, u64, &'s str)>> {
    let same = |a: (&str, &str), b: (&str, &str)| a == b;
    site.locals
        .iter()
        .enumerate()
        .map(|(position, local)| {
            let key = (local.name.as_str(), local.scope.as_str());
            // A block that redeclares a name: counted from the innermost.
            let later = site.locals[position + 1..]
                .iter()
                .filter(|other| same((other.name.as_str(), other.scope.as_str()), key))
                .count();
            old.iter()
                .rev()
                .filter(|was| same((was.name.as_str(), was.scope.as_str()), key))
                .nth(later)
                .filter(|was| was.ty == local.ty)
                .map(|was| (local.register, was.register, local.ty.as_str()))
        })
        .collect()
}

/// The body run again from its entry, with the arguments it was given.
fn restart(frozen: &FrozenMachine, program: &Program) -> Option<Machine> {
    let name = &frozen.frames.first()?.function;
    let function = program.function(name)?;
    let mut held: Vec<crate::arena::Owned> = Vec::new();
    let arguments = thaw_arguments(frozen, program, &mut held)?;
    // The parameters as declared, where the compiler recorded them.
    if let Some(entry) = function
        .sites
        .iter()
        .find(|site| site.kind == SiteKind::Entry)
    {
        let parameters: Vec<_> = entry
            .locals
            .iter()
            .filter(|local| usize::from(local.register) < function.arity)
            .collect();
        if parameters.len() != frozen.arguments.len()
            || !parameters
                .iter()
                .zip(&frozen.arguments)
                .all(|(parameter, value)| fits(value, &parameter.ty, &program.structs))
        {
            return None;
        }
    }
    if !super::freeze::holds_all(&held, program) {
        return None;
    }
    let mut machine = Machine::new(program, name, &arguments)?;
    machine.held = held;
    Some(machine)
}

/// Whether a local of type `ty` can hold `value`: what [`fits`] says, an
/// `int` in a float-typed local too — the VM widens one where a float is read,
/// so `float x = 1;` holds an `Int` — and nothing written yet.
fn holds(value: &FrozenValue, ty: &str, structs: &[crate::vm::StructLayout]) -> bool {
    let float = matches!(ty.trim_end_matches('?'), "float" | "Duration" | "Angle");
    // `Unit` is a local not written yet — `int count;` before its first
    // assignment — which any local can be.
    matches!(value, FrozenValue::Unit)
        || fits(value, ty, structs)
        || (float && matches!(value, FrozenValue::Int(_)))
}

/// Whether `value` can be a `ty`, as far as its kind says.
///
/// A type whose values freeze as nothing more specific than their kind — a
/// struct, an array — is taken on trust.
fn fits(value: &FrozenValue, ty: &str, structs: &[crate::vm::StructLayout]) -> bool {
    if let Some(inner) = ty.strip_suffix('?') {
        return matches!(value, FrozenValue::Null) || fits(value, inner, structs);
    }
    if let Some(element) = ty.strip_suffix("[]") {
        return match value {
            FrozenValue::Array(items) => items.iter().all(|item| holds(item, element, structs)),
            _ => false,
        };
    }
    // A struct by its name; its fields are matched by name when it is
    // imported into the running program.
    if let FrozenValue::Struct { name, fields } = value {
        let Some(layout) = structs.iter().find(|layout| layout.name == *name) else {
            return false;
        };
        return name == ty
            && fields.iter().all(|(field, value)| {
                layout
                    .fields
                    .iter()
                    .find(|(declared, _)| declared == field)
                    .is_none_or(|(_, declared)| holds(value, declared, structs))
            });
    }
    match ty {
        "int" => matches!(value, FrozenValue::Int(_)),
        "float" | "Duration" | "Angle" => matches!(value, FrozenValue::Float(_)),
        "bool" => matches!(value, FrozenValue::Bool(_)),
        "string" => matches!(
            value,
            FrozenValue::Literal(_) | FrozenValue::Text(_) | FrozenValue::Expired
        ),
        "Entity" => matches!(value, FrozenValue::Entity(_)),
        "Vec2" => matches!(value, FrozenValue::Vec2(_)),
        "Vec3" => matches!(value, FrozenValue::Vec3(_)),
        "Vec4" => matches!(value, FrozenValue::Vec4(_)),
        "Quat" => matches!(value, FrozenValue::Quat(_)),
        "Color" => matches!(value, FrozenValue::Color(_)),
        _ => true,
    }
}
