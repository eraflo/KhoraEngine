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

//! What the compiler works out once every function exists: each function's
//! fingerprint, and the most a run can overdraw.
//!
//! A fingerprint is blake3 over a function's name, arity, frame size and code,
//! **normalized**: an operand that names something outside the function — a
//! callee, a native, a literal, a field — is hashed by what it names, never by
//! the index the program happens to give it. Reordering functions, adding a
//! literal elsewhere or a field before the one it reads leaves it alone; an
//! edit to its own code changes it. Registers and jump targets are the
//! function's own and are hashed as they are.

use crate::native::NativeRegistry;
use crate::vm::program::first_u64;
use crate::vm::{BehaviorLayout, Function, Instruction, Program, StrRef, Value};

/// Fingerprints every function of `program` and records its overdraft bound.
pub(super) fn seal(program: &mut Program, natives: &NativeRegistry) {
    let fingerprints: Vec<u64> = program
        .functions
        .iter()
        .map(|function| fingerprint(function, program, natives))
        .collect();
    for (function, fingerprint) in program.functions.iter_mut().zip(fingerprints) {
        function.fingerprint = fingerprint;
    }
    program.max_overdraft = program
        .functions
        .iter()
        .map(|function| longest_stretch(function, natives))
        .max()
        .unwrap_or(0);
}

/// One function's fingerprint, against the program and natives it was
/// compiled with.
fn fingerprint(function: &Function, program: &Program, natives: &NativeRegistry) -> u64 {
    let layout = function
        .name
        .split_once('.')
        .and_then(|(behavior, _)| program.layout(behavior));
    let mut hash = Hash::new();
    hash.text(&function.name);
    hash.number(function.arity as u64);
    hash.number(function.registers as u64);
    hash.number(function.code.len() as u64);
    for instruction in &function.code {
        hash.instruction(instruction, program, natives, layout);
    }
    first_u64(hash.0.finalize().as_bytes())
}

/// What a field slot is, by name: the slot index moves when a field is added
/// before it, the name does not.
fn slot_name(layout: Option<&BehaviorLayout>, slot: u16) -> String {
    let slot = usize::from(slot);
    let Some(layout) = layout else {
        return format!("slot#{slot}");
    };
    if let Some(field) = layout.fields.get(slot) {
        return format!("field:{field}");
    }
    if !layout.states.is_empty() && slot == layout.state_slot() {
        return "state".to_owned();
    }
    let data = layout.state_data_slot();
    let timers = layout.timer_slot(0);
    if (data..timers).contains(&slot) {
        // A state's data is shared by every state, so it is named by its
        // offset in that region: the code reads it the same way.
        return format!("data[{}]", slot - data);
    }
    match slot
        .checked_sub(timers)
        .and_then(|index| layout.timers.get(index))
    {
        Some(timer) => format!("timer:{}", timer.member),
        None => format!("slot#{slot}"),
    }
}

/// A blake3 hasher with the encodings a fingerprint needs.
struct Hash(blake3::Hasher);

impl Hash {
    fn new() -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"khora.ergon.function");
        Self(hasher)
    }

    fn number(&mut self, value: u64) {
        self.0.update(&value.to_le_bytes());
    }

    /// A component write, by the component's name and its fields' names.
    fn patch(&mut self, program: &Program, patch: u32) {
        let declared = program.patches.get(patch as usize);
        self.text(declared.map_or("", |declared| declared.component.as_str()));
        let fields = declared
            .map(|declared| declared.fields.as_slice())
            .unwrap_or(&[]);
        self.number(fields.len() as u64);
        for field in fields {
            self.text(field);
        }
    }

    /// A struct, by its name — and one of its fields, by its name.
    fn struct_layout(&mut self, program: &Program, layout: u16, slot: Option<u16>) {
        let declared = program.structs.get(usize::from(layout));
        self.text(declared.map_or("", |declared| declared.name.as_str()));
        match slot {
            Some(slot) => self.text(
                declared
                    .and_then(|declared| declared.fields.get(usize::from(slot)))
                    .map_or("", |(field, _)| field.as_str()),
            ),
            None => {
                for (field, _) in declared
                    .map(|declared| declared.fields.as_slice())
                    .unwrap_or(&[])
                {
                    self.text(field);
                }
            }
        }
    }

    fn text(&mut self, text: &str) {
        self.number(text.len() as u64);
        self.0.update(text.as_bytes());
    }

    fn floats(&mut self, values: &[f32]) {
        for value in values {
            self.number(u64::from(value.to_bits()));
        }
    }

    /// Writes an opcode and its register operands.
    fn op(&mut self, opcode: &str, registers: &[u8]) {
        self.text(opcode);
        self.0.update(registers);
    }

    fn value(&mut self, value: &Value, program: &Program) {
        match value {
            Value::Unit => self.text("unit"),
            Value::Int(value) => {
                self.text("int");
                self.number(*value as u64);
            }
            Value::Float(value) => {
                self.text("float");
                self.floats(&[*value]);
            }
            Value::Bool(value) => {
                self.text("bool");
                self.number(u64::from(*value));
            }
            Value::Entity(entity) => {
                self.text("entity");
                self.number(u64::from(entity.index));
                self.number(u64::from(entity.generation));
            }
            Value::Str(StrRef::Const(index)) => {
                self.text("str");
                self.text(program.string(*index).unwrap_or_default());
            }
            Value::Str(StrRef::Arena(_)) => self.text("arena"),
            // Neither is ever a compiled constant; named only so the match
            // stays exhaustive.
            Value::Str(StrRef::Held(_)) => self.text("held"),
            Value::Obj(_) => self.text("obj"),
            Value::Vec2(v) => {
                self.text("vec2");
                self.floats(&[v.x, v.y]);
            }
            Value::Vec3(v) => {
                self.text("vec3");
                self.floats(&[v.x, v.y, v.z]);
            }
            Value::Vec4(v) => {
                self.text("vec4");
                self.floats(&[v.x, v.y, v.z, v.w]);
            }
            Value::Quat(q) => {
                self.text("quat");
                self.floats(&[q.x, q.y, q.z, q.w]);
            }
            Value::Color(c) => {
                self.text("color");
                self.floats(&[c.r, c.g, c.b, c.a]);
            }
            Value::Null => self.text("null"),
        }
    }

    fn instruction(
        &mut self,
        instruction: &Instruction,
        program: &Program,
        natives: &NativeRegistry,
        layout: Option<&BehaviorLayout>,
    ) {
        use Instruction as I;
        match instruction {
            I::LoadConst { dst, value } => {
                self.op("LoadConst", &[*dst]);
                self.value(value, program);
            }
            I::Move { dst, src } => self.op("Move", &[*dst, *src]),
            I::AddInt { dst, lhs, rhs } => self.op("AddInt", &[*dst, *lhs, *rhs]),
            I::SubInt { dst, lhs, rhs } => self.op("SubInt", &[*dst, *lhs, *rhs]),
            I::MulInt { dst, lhs, rhs } => self.op("MulInt", &[*dst, *lhs, *rhs]),
            I::DivInt { dst, lhs, rhs } => self.op("DivInt", &[*dst, *lhs, *rhs]),
            I::RemInt { dst, lhs, rhs } => self.op("RemInt", &[*dst, *lhs, *rhs]),
            I::NegInt { dst, src } => self.op("NegInt", &[*dst, *src]),
            I::AddFloat { dst, lhs, rhs } => self.op("AddFloat", &[*dst, *lhs, *rhs]),
            I::SubFloat { dst, lhs, rhs } => self.op("SubFloat", &[*dst, *lhs, *rhs]),
            I::MulFloat { dst, lhs, rhs } => self.op("MulFloat", &[*dst, *lhs, *rhs]),
            I::DivFloat { dst, lhs, rhs } => self.op("DivFloat", &[*dst, *lhs, *rhs]),
            I::NegFloat { dst, src } => self.op("NegFloat", &[*dst, *src]),
            I::IntToFloat { dst, src } => self.op("IntToFloat", &[*dst, *src]),
            I::FloatToInt { dst, src } => self.op("FloatToInt", &[*dst, *src]),
            I::Eq { dst, lhs, rhs } => self.op("Eq", &[*dst, *lhs, *rhs]),
            I::Less { dst, lhs, rhs } => self.op("Less", &[*dst, *lhs, *rhs]),
            I::LessEq { dst, lhs, rhs } => self.op("LessEq", &[*dst, *lhs, *rhs]),
            I::Not { dst, src } => self.op("Not", &[*dst, *src]),
            I::IsNull { dst, src } => self.op("IsNull", &[*dst, *src]),
            I::Jump { target } => {
                self.op("Jump", &[]);
                self.number(*target as u64);
            }
            I::JumpIfNot { cond, target } => {
                self.op("JumpIfNot", &[*cond]);
                self.number(*target as u64);
            }
            I::NewArray { dst, base, count } => {
                self.op("NewArray", &[*dst, *base]);
                self.number(u64::from(*count));
            }
            I::Extend { array, base, count } => {
                self.op("Extend", &[*array, *base]);
                self.number(u64::from(*count));
            }
            I::GetIndex { dst, object, index } => self.op("GetIndex", &[*dst, *object, *index]),
            I::SetIndex { object, index, src } => self.op("SetIndex", &[*object, *index, *src]),
            I::Length { dst, src } => self.op("Length", &[*dst, *src]),
            I::Copy { dst, src } => self.op("Copy", &[*dst, *src]),
            I::Push { array, src } => self.op("Push", &[*array, *src]),
            I::RemoveAt {
                array,
                index,
                charged,
                from,
            } => {
                self.op("RemoveAt", &[*array, *index]);
                self.number(u64::from(*charged));
                self.number(*from as u64);
            }
            // A struct by its name and its fields', never by index: a struct
            // declared elsewhere, or a field moved, is the same code.
            I::NewStruct {
                dst,
                layout,
                base,
                count,
            } => {
                self.op("NewStruct", &[*dst, *base]);
                self.number(u64::from(*count));
                self.struct_layout(program, *layout, None);
            }
            I::GetField {
                dst,
                object,
                layout,
                slot,
            } => {
                self.op("GetField", &[*dst, *object]);
                self.struct_layout(program, *layout, Some(*slot));
            }
            I::SetField {
                object,
                layout,
                slot,
                src,
            } => {
                self.op("SetField", &[*object, *src]);
                self.struct_layout(program, *layout, Some(*slot));
            }
            I::JumpIfNull { src, target } => {
                self.op("JumpIfNull", &[*src]);
                self.number(*target as u64);
            }
            I::Call {
                function,
                base,
                argc,
                dst,
            } => {
                self.op("Call", &[*base, *argc, *dst]);
                self.text(
                    program
                        .functions
                        .get(*function)
                        .map_or("", |callee| callee.name.as_str()),
                );
            }
            I::Return { src } => self.op("Return", &[*src]),
            I::Become {
                state,
                base,
                argc,
                state_slot,
                data_slot,
            } => {
                self.op("Become", &[*base, *argc]);
                self.text(
                    layout
                        .and_then(|layout| layout.state_at(usize::from(*state)))
                        .map_or("", |state| state.name.as_str()),
                );
                self.text(&slot_name(layout, *state_slot));
                self.text(&slot_name(layout, *data_slot));
            }
            I::LoadField { dst, slot } => {
                self.op("LoadField", &[*dst]);
                self.text(&slot_name(layout, *slot));
            }
            I::StoreField { slot, src } => {
                self.op("StoreField", &[*src]);
                self.text(&slot_name(layout, *slot));
            }
            I::WriteBack { slot, src } => {
                self.op("WriteBack", &[*src]);
                self.text(&slot_name(layout, *slot));
            }
            I::LoadStr { dst, index } => {
                self.op("LoadStr", &[*dst]);
                self.text(program.string(*index).unwrap_or_default());
            }
            I::Concat { dst, lhs, rhs } => self.op("Concat", &[*dst, *lhs, *rhs]),
            I::NativeCall {
                function,
                base,
                argc,
                dst,
            } => {
                self.op("NativeCall", &[*base, *argc, *dst]);
                self.text(natives.at(*function).map_or("", |native| native.name));
            }
            I::WriteComponent {
                mode,
                entity,
                patch,
                base,
                count,
            } => {
                self.op("WriteComponent", &[*entity, *base]);
                self.text(match mode {
                    crate::vm::WriteMode::Set => "set",
                    crate::vm::WriteMode::Add => "add",
                });
                self.number(u64::from(*count));
                self.patch(program, *patch);
            }
            I::RemoveComponent { entity, component } => {
                self.op("RemoveComponent", &[*entity]);
                self.text(program.string(*component).unwrap_or_default());
            }
            I::SpawnEntity {
                position,
                spawn,
                base,
                count,
            } => {
                self.op("SpawnEntity", &[*position, *base]);
                self.number(u64::from(*count));
                let patches = program
                    .spawns
                    .get(*spawn as usize)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                self.number(patches.len() as u64);
                for patch in patches {
                    self.patch(program, *patch);
                }
            }
            I::Await { seconds, dst } => self.op("Await", &[*seconds, *dst]),
            I::LoadSelf { dst } => self.op("LoadSelf", &[*dst]),
            I::Safepoint => self.op("Safepoint", &[]),
            I::Yield => self.op("Yield", &[]),
            I::Halt => self.op("Halt", &[]),
        }
    }
}

/// The costliest run of `function`'s code between two places the machine
/// checks its fuel, natives at their declared cost.
///
/// A stretch ends at a safepoint, and at an instruction after which the
/// machine checks again or stops: a call (its callee's entry), a return (the
/// caller's return site), an `await`, a yield, a halt. Back edges land on
/// safepoints, so the code between two of them is acyclic and its longest
/// path is found by walking forward.
fn longest_stretch(function: &Function, natives: &NativeRegistry) -> u64 {
    let code = &function.code;
    // `None` while unvisited, `Some(None)` while being walked. Every cycle goes
    // through a backward jump, which ends a stretch, so the walk never meets
    // itself; the marker only keeps a malformed program from recursing.
    let mut memo: Vec<Option<Option<u64>>> = vec![None; code.len()];

    fn walk(
        pc: usize,
        code: &[Instruction],
        natives: &NativeRegistry,
        memo: &mut [Option<Option<u64>>],
    ) -> u64 {
        let Some(instruction) = code.get(pc) else {
            return 0;
        };
        match memo[pc] {
            Some(Some(cost)) => return cost,
            Some(None) => return u64::MAX,
            None => {}
        }
        memo[pc] = Some(None);

        let cost = match instruction {
            Instruction::NativeCall { function, .. } => {
                natives.at(*function).map_or(1, |native| native.cost)
            }
            other => other.cost(),
        };
        let next = |target: usize, memo: &mut [Option<Option<u64>>]| -> u64 {
            match code.get(target) {
                None | Some(Instruction::Safepoint) => 0,
                Some(_) => walk(target, code, natives, memo),
            }
        };
        let rest = match instruction {
            Instruction::Call { .. }
            | Instruction::Return { .. }
            | Instruction::Await { .. }
            | Instruction::Yield
            | Instruction::Halt => 0,
            // A jump backwards lands where the machine checks again.
            Instruction::Jump { target } if *target <= pc => 0,
            Instruction::Jump { target } => next(*target, memo),
            Instruction::JumpIfNot { target, .. } | Instruction::JumpIfNull { target, .. }
                if *target <= pc =>
            {
                next(pc + 1, memo)
            }
            Instruction::JumpIfNot { target, .. } | Instruction::JumpIfNull { target, .. } => {
                next(pc + 1, memo).max(next(*target, memo))
            }
            _ => next(pc + 1, memo),
        };
        let total = cost.saturating_add(rest);
        memo[pc] = Some(Some(total));
        total
    }

    (0..code.len())
        .map(|pc| walk(pc, code, natives, &mut memo))
        .max()
        .unwrap_or(0)
}
