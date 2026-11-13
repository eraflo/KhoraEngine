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

//! One instruction's execution: [`Machine::step`] dispatches on the
//! instruction, and each arm with work of its own runs in a method below it.
//! Arrays, component commands, rows and formatting run in their own files.

use super::{Fault, Instruction, Machine, Program, Reg, StrRef, Value};
use crate::arena::Persisted;
use crate::native::Host;

/// How the program counter moves after an instruction.
pub(super) enum Step {
    /// Advance one.
    Next,
    /// The instruction already set it.
    Jumped,
    /// Stop for good.
    Halt,
    /// Stop, resumable.
    Yield,
    /// A frame was popped.
    Returned,
}

impl Machine {
    /// Executes one instruction.
    pub(super) fn step(
        &mut self,
        instruction: &Instruction,
        program: &Program,
        host: &mut Host,
        code_len: usize,
    ) -> Result<Step, Fault> {
        match *instruction {
            Instruction::LoadConst { dst, value } => {
                self.write(dst, value)?;
                Ok(Step::Next)
            }
            Instruction::Move { dst, src } => {
                let value = self.read(src)?;
                self.write(dst, value)?;
                Ok(Step::Next)
            }

            Instruction::AddInt { dst, lhs, rhs } => self.int_op(dst, lhs, rhs, i64::wrapping_add),
            Instruction::SubInt { dst, lhs, rhs } => self.int_op(dst, lhs, rhs, i64::wrapping_sub),
            Instruction::MulInt { dst, lhs, rhs } => self.int_op(dst, lhs, rhs, i64::wrapping_mul),
            Instruction::DivInt { dst, lhs, rhs } => {
                self.checked_int_op(dst, lhs, rhs, |a, b| (b != 0).then(|| a.wrapping_div(b)))
            }
            Instruction::RemInt { dst, lhs, rhs } => {
                self.checked_int_op(dst, lhs, rhs, |a, b| (b != 0).then(|| a.wrapping_rem(b)))
            }
            Instruction::NegInt { dst, src } => self.neg_int(dst, src),

            Instruction::AddFloat { dst, lhs, rhs } => self.float_op(dst, lhs, rhs, |a, b| a + b),
            Instruction::SubFloat { dst, lhs, rhs } => self.float_op(dst, lhs, rhs, |a, b| a - b),
            Instruction::MulFloat { dst, lhs, rhs } => self.float_op(dst, lhs, rhs, |a, b| a * b),
            Instruction::DivFloat { dst, lhs, rhs } => self.float_op(dst, lhs, rhs, |a, b| a / b),
            Instruction::NegFloat { dst, src } => self.neg_float(dst, src),
            Instruction::IntToFloat { dst, src } => self.int_to_float(dst, src),
            Instruction::FloatToInt { dst, src } => self.float_to_int(dst, src),

            Instruction::Eq { dst, lhs, rhs } => self.equal(dst, lhs, rhs, program, host),
            Instruction::Less { dst, lhs, rhs } => self.compare(dst, lhs, rhs, true),
            Instruction::LessEq { dst, lhs, rhs } => self.compare(dst, lhs, rhs, false),
            Instruction::Not { dst, src } => self.not(dst, src),
            Instruction::IsNull { dst, src } => {
                let value = self.read(src)?;
                self.write(dst, Value::Bool(value.is_null()))?;
                Ok(Step::Next)
            }

            Instruction::Jump { target } => {
                self.jump_to(target, code_len)?;
                Ok(Step::Jumped)
            }
            Instruction::JumpIfNot { cond, target } => self.jump_if_not(cond, target, code_len),
            Instruction::JumpIfNull { src, target } => {
                if self.read(src)?.is_null() {
                    self.jump_to(target, code_len)?;
                    Ok(Step::Jumped)
                } else {
                    Ok(Step::Next)
                }
            }

            Instruction::Call {
                function,
                base,
                argc,
                dst,
            } => self.call(program, function, base, argc, dst),
            Instruction::Return { src } => {
                let value = self.read(src)?;
                // Whether a caller took the value or the program ended is
                // decided by `run`, which checks whether any frame is left.
                self.pop_frame(value);
                Ok(Step::Returned)
            }

            Instruction::Become { .. } => self.step_become(instruction, program, host),

            Instruction::LoadField { dst, slot } => self.load_field(dst, slot, program, host),
            Instruction::StoreField { slot, src } | Instruction::WriteBack { slot, src } => {
                let value = self.read(src)?;
                let stored = self.persist(value, program, host)?;
                host.fields.set(slot as usize, stored);
                Ok(Step::Next)
            }

            Instruction::LoadSelf { dst } => {
                let entity = host.entity.ok_or(Fault::NoSubject)?;
                self.write(dst, Value::Entity(entity))?;
                Ok(Step::Next)
            }
            Instruction::LoadStr { dst, index } => {
                if program.string(index).is_none() {
                    return Err(Fault::BadString);
                }
                self.write(dst, Value::Str(StrRef::Const(index)))?;
                Ok(Step::Next)
            }
            Instruction::NewArray { .. }
            | Instruction::Extend { .. }
            | Instruction::GetIndex { .. }
            | Instruction::SetIndex { .. }
            | Instruction::Length { .. }
            | Instruction::Copy { .. }
            | Instruction::NewStruct { .. }
            | Instruction::GetField { .. }
            | Instruction::SetField { .. }
            | Instruction::Push { .. }
            | Instruction::RemoveAt { .. } => self.step_object(instruction, program, host),
            Instruction::WriteComponent { .. }
            | Instruction::GetComponent { .. }
            | Instruction::HasComponent { .. }
            | Instruction::QueryRows { .. }
            | Instruction::QueryHolds { .. }
            | Instruction::RemoveComponent { .. }
            | Instruction::SetMaterial { .. }
            | Instruction::Raise { .. }
            | Instruction::SpawnEntity { .. }
            | Instruction::SpawnPrefab { .. } => self.step_command(instruction, program, host),
            Instruction::WriteRow { .. } => self.step_write_row(instruction, program, host),

            Instruction::Format {
                dst, base, plan, ..
            } => {
                self.step_format(program, host, dst, base, plan)?;
                Ok(Step::Next)
            }

            Instruction::Concat { dst, lhs, rhs } => self.concat(dst, lhs, rhs, program, host),

            Instruction::NativeCall {
                function,
                base,
                argc,
                dst,
            } => self.native_call(program, host, function, base, argc, dst),

            Instruction::Await { seconds, dst } => self.await_for(seconds, dst, host),

            Instruction::Safepoint => Ok(Step::Next),
            Instruction::Yield => Ok(Step::Yield),
            Instruction::Halt => Ok(Step::Halt),
        }
    }

    /// `NegInt`: the integer in `src`, negated, into `dst`.
    fn neg_int(&mut self, dst: Reg, src: Reg) -> Result<Step, Fault> {
        let value = self.int(src)?;
        self.write(dst, Value::Int(value.wrapping_neg()))?;
        Ok(Step::Next)
    }

    /// `NegFloat`: the float in `src`, negated, into `dst`.
    fn neg_float(&mut self, dst: Reg, src: Reg) -> Result<Step, Fault> {
        let value = self.float(src)?;
        self.write(dst, Value::Float(-value))?;
        Ok(Step::Next)
    }

    /// `IntToFloat`: the number in `src`, as a float, into `dst`.
    fn int_to_float(&mut self, dst: Reg, src: Reg) -> Result<Step, Fault> {
        let value = self.float(src)?;
        self.write(dst, Value::Float(value))?;
        Ok(Step::Next)
    }

    /// `FloatToInt`: the float in `src`, truncated, into `dst`.
    fn float_to_int(&mut self, dst: Reg, src: Reg) -> Result<Step, Fault> {
        let value = self.float(src)?;
        // Every float in `[-2^63, 2^63)` truncates to an `i64`; past it,
        // or not a number at all, there is no `int` to give — and a
        // saturated one would be a wrong answer nobody asked for.
        const BOUND: f64 = 9_223_372_036_854_775_808.0;
        let wide = f64::from(value);
        if !wide.is_finite() || !(-BOUND..BOUND).contains(&wide) {
            return Err(Fault::InvalidCast {
                value: value.to_string(),
            });
        }
        self.write(dst, Value::Int(wide.trunc() as i64))?;
        Ok(Step::Next)
    }

    /// `Eq`: whether `lhs` and `rhs` hold the same value, into `dst`.
    fn equal(
        &mut self,
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
        program: &Program,
        host: &Host,
    ) -> Result<Step, Fault> {
        let a = self.read(lhs)?;
        let b = self.read(rhs)?;
        // Comparing an int against a float compares their numeric
        // value; the checker only allows it where that is meant.
        let equal = match (a, b) {
            (Value::Int(x), Value::Float(y)) | (Value::Float(y), Value::Int(x)) => x as f32 == y,
            // Two strings are equal when their *text* is. Comparing the
            // references would answer no for a literal and a computed
            // string spelling the same thing, which is the one case a
            // script most often means to compare.
            (Value::Str(_), Value::Str(_)) => {
                self.resolve_str(a, program, host)? == self.resolve_str(b, program, host)?
            }
            _ => a == b,
        };
        self.write(dst, Value::Bool(equal))?;
        Ok(Step::Next)
    }

    /// `Not`: the bool in `src`, negated, into `dst`.
    fn not(&mut self, dst: Reg, src: Reg) -> Result<Step, Fault> {
        let value = self.read(src)?;
        let flag = value.as_bool().ok_or(Fault::TypeMismatch {
            expected: "bool",
            found: value.type_name(),
        })?;
        self.write(dst, Value::Bool(!flag))?;
        Ok(Step::Next)
    }

    /// `JumpIfNot`: a jump to `target` when the bool in `cond` is false.
    fn jump_if_not(&mut self, cond: Reg, target: usize, code_len: usize) -> Result<Step, Fault> {
        let value = self.read(cond)?;
        let taken = value.as_bool().ok_or(Fault::TypeMismatch {
            expected: "bool",
            found: value.type_name(),
        })?;
        if taken {
            Ok(Step::Next)
        } else {
            self.jump_to(target, code_len)?;
            Ok(Step::Jumped)
        }
    }

    /// `Become`: enters a behavior's state, with its arguments.
    fn step_become(
        &mut self,
        instruction: &Instruction,
        program: &Program,
        host: &mut Host,
    ) -> Result<Step, Fault> {
        let Instruction::Become {
            behavior,
            writes: _,
            state,
            base,
            argc,
            state_slot,
            data_slot,
        } = *instruction
        else {
            unreachable!("step_become is reached only for `Become`")
        };
        // The arguments first, read while the old state's data is still
        // there: an argument may have been read out of it. Then the
        // entered state's data at its defaults and its countdowns armed
        // — data the layout holds — then the arguments into their
        // slots, and the discriminant last. Nothing here can fail
        // part-way, so a state is never half-entered.
        let mut arguments = Vec::with_capacity(usize::from(argc));
        for offset in 0..argc {
            let value = self.read(base + offset)?;
            arguments.push(self.persist(value, program, host)?);
        }
        if let Some(layout) = program.behaviors.get(usize::from(behavior)) {
            for (slot, value) in layout.entry(usize::from(state)) {
                host.fields.set(slot, value);
            }
        }
        for (offset, stored) in arguments.into_iter().enumerate() {
            host.fields.set(data_slot as usize + offset, stored);
        }
        host.fields.set(
            state_slot as usize,
            Persisted::Scalar(Value::Int(state as i64)),
        );
        Ok(Step::Next)
    }

    /// `LoadField`: one of the running behavior's fields into `dst`.
    fn load_field(
        &mut self,
        dst: Reg,
        slot: u16,
        program: &Program,
        host: &mut Host,
    ) -> Result<Step, Fault> {
        let value = match host.fields.get(slot as usize) {
            Some(Persisted::Scalar(value)) => *value,
            // Text kept in a field is stored by value, so reading it
            // brings a copy into the frame arena — where everything the
            // running code can name lives.
            Some(Persisted::Owned(owned)) => host
                .arena
                .import(owned, program)
                .map_err(|_| Fault::ArenaFull)?,
            // A slot the instance does not have yet: a script that
            // gained a field since this entity was saved. Unset rather
            // than a fault — hot-reload is meant to survive that.
            None => Value::Unit,
        };
        self.write(dst, value)?;
        Ok(Step::Next)
    }

    /// `Concat`: the text of `lhs` then `rhs`, as a new arena string, into
    /// `dst`.
    fn concat(
        &mut self,
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
        program: &Program,
        host: &mut Host,
    ) -> Result<Step, Fault> {
        let left = self.read(lhs)?;
        let right = self.read(rhs)?;
        let joined = format!(
            "{}{}",
            self.resolve_str(left, program, host)?,
            self.resolve_str(right, program, host)?
        );
        let reference = host
            .arena
            .alloc(crate::arena::Object::Str(joined))
            .map_err(|_| Fault::ArenaFull)?;
        self.write(dst, Value::Str(StrRef::Arena(reference)))?;
        Ok(Step::Next)
    }

    /// `Await`: yields for the duration in `seconds`, with `dst` already
    /// written for the resume.
    fn await_for(&mut self, seconds: Reg, dst: Reg, host: &mut Host) -> Result<Step, Fault> {
        let value = self.read(seconds)?;
        let wait = value.as_float().ok_or(Fault::TypeMismatch {
            expected: "Duration",
            found: value.type_name(),
        })?;
        // Written before suspending, so resuming finds it already there
        // and does not have to know an `await` was in progress.
        self.write(dst, Value::Unit)?;
        // The caller reads this to decide when to come back. Left on the
        // host rather than returned, because the suspension itself is
        // the same one the budget produces — only the reason differs.
        host.awaiting = Some(wait.max(0.0));
        Ok(Step::Yield)
    }

    /// Runs an engine function and writes its result back.
    ///
    /// No frame is pushed: a native has no bytecode to give registers to, and
    /// nothing to return into. That is also why it is the one place a run
    /// cannot be suspended part-way — the call either completes or faults.
    fn native_call(
        &mut self,
        program: &Program,
        host: &mut Host,
        function: usize,
        base: Reg,
        argc: u8,
        dst: Reg,
    ) -> Result<Step, Fault> {
        let native = host
            .natives
            .at(function)
            .ok_or(Fault::UnknownNative { index: function })?;

        // Copied out rather than borrowed: the native takes the host mutably,
        // and the arguments live in the machine's register file, so holding a
        // slice across the call would borrow both at once.
        let mut args = Vec::with_capacity(argc as usize);
        for offset in 0..argc {
            args.push(self.read(base + offset)?);
        }

        let mut context = host.context(&program.strings);
        let result = (native.call)(&mut context, &args).map_err(|error| Fault::NativeFailed {
            name: native.name,
            message: error.message,
        })?;

        self.write(dst, result)?;
        Ok(Step::Next)
    }

    fn int_op(
        &mut self,
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
        op: fn(i64, i64) -> i64,
    ) -> Result<Step, Fault> {
        let a = self.int(lhs)?;
        let b = self.int(rhs)?;
        self.write(dst, Value::Int(op(a, b)))?;
        Ok(Step::Next)
    }

    /// An integer operation that can refuse its operands — division by zero.
    fn checked_int_op(
        &mut self,
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
        op: fn(i64, i64) -> Option<i64>,
    ) -> Result<Step, Fault> {
        let a = self.int(lhs)?;
        let b = self.int(rhs)?;
        let value = op(a, b).ok_or(Fault::DivideByZero)?;
        self.write(dst, Value::Int(value))?;
        Ok(Step::Next)
    }

    fn float_op(
        &mut self,
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
        op: fn(f32, f32) -> f32,
    ) -> Result<Step, Fault> {
        let a = self.float(lhs)?;
        let b = self.float(rhs)?;
        self.write(dst, Value::Float(op(a, b)))?;
        Ok(Step::Next)
    }

    /// `lhs < rhs` when `strict`, `lhs <= rhs` otherwise.
    ///
    /// Two integers compare as integers: routing them through `f32` would lose
    /// precision above 2^24, and a comparison that goes wrong on large counters
    /// is the kind of bug nobody looks for.
    fn compare(&mut self, dst: Reg, lhs: Reg, rhs: Reg, strict: bool) -> Result<Step, Fault> {
        let a = self.read(lhs)?;
        let b = self.read(rhs)?;

        let result = match (a.as_int(), b.as_int()) {
            (Some(x), Some(y)) => {
                if strict {
                    x < y
                } else {
                    x <= y
                }
            }
            _ => {
                let x = a.as_float().ok_or(Fault::TypeMismatch {
                    expected: "a number",
                    found: a.type_name(),
                })?;
                let y = b.as_float().ok_or(Fault::TypeMismatch {
                    expected: "a number",
                    found: b.type_name(),
                })?;
                if strict {
                    x < y
                } else {
                    x <= y
                }
            }
        };

        self.write(dst, Value::Bool(result))?;
        Ok(Step::Next)
    }
}
