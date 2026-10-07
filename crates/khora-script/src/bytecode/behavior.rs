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

//! Compiling a behavior: its layout, its members, its states and the code that
//! gives a fresh instance its values.
//!
//! A behavior compiles to plain functions — `Guard.Update`, `Guard.Patrol.OnHit`
//! — plus a layout naming its slots. What makes a member a *member* is the
//! field table in scope while it compiles, and which state's data it can name.

use super::sites;
use super::{
    init_name, keys, member_body, member_signature, shape_of, shape_of_state_slot, Compiler,
    Registers, Shape, StateEntry,
};
use crate::ast::BehaviorMember;
use crate::vm::{Function, Instruction};

impl Compiler {
    /// Compiles a behavior's members, each as a function of its own.
    ///
    /// A member is not special at run time: `on Damaged(int amount)` becomes a
    /// function taking an `int`, named `Guard.Damaged` so the dispatcher can
    /// find it. What makes it a *member* is the field table in scope while it
    /// compiles — which is why the fields are assigned slots first, before any
    /// body is read, so a member can name a field declared below it.
    pub(super) fn compile_behavior(&mut self, decl: &crate::ast::BehaviorDecl) {
        self.fields.clear();
        let mut layout = crate::vm::BehaviorLayout {
            name: decl.name.clone(),
            fields: Vec::new(),
            field_types: Vec::new(),
            states: Vec::new(),
            timers: Vec::new(),
        };
        for member in &decl.members {
            if let BehaviorMember::Field(field) = member {
                let slot = self.fields.len() as u16;
                self.fields
                    .insert(field.name.clone(), (slot, shape_of(&field.ty)));
                layout.fields.push(field.name.clone());
                layout.field_types.push(keys::type_name(&field.ty));
            }
        }
        self.behavior_fields = self.fields.clone();

        self.collect_timers(&mut layout, &decl.name, None, &decl.members);

        // States before any body: `become Chase(…)` written inside `Patrol`
        // has to resolve a state declared below it, and a file's order should
        // not decide what compiles.
        for member in &decl.members {
            if let BehaviorMember::State(state) = member {
                layout.states.push(crate::vm::StateLayout {
                    name: state.name.clone(),
                    slots: state
                        .params
                        .iter()
                        .map(|param| param.name.clone())
                        .chain(state.members.iter().filter_map(|member| match member {
                            BehaviorMember::Field(field) => Some(field.name.clone()),
                            _ => None,
                        }))
                        .collect(),
                    types: state
                        .params
                        .iter()
                        .map(|param| keys::type_name(&param.ty))
                        .chain(state.members.iter().filter_map(|member| match member {
                            BehaviorMember::Field(field) => Some(keys::type_name(&field.ty)),
                            _ => None,
                        }))
                        .collect(),
                });
            }
        }

        // A state's own schedules, after its slots exist so the discriminant is
        // known. `every 0.5s` written inside `Patrol` was collected by nobody
        // before this, so it compiled and never fired.
        for member in &decl.members {
            let BehaviorMember::State(state) = member else {
                continue;
            };
            let Some(index) = layout.state_index(&state.name) else {
                continue;
            };
            let name = state.name.clone();
            self.collect_timers(
                &mut layout,
                &decl.name,
                Some((index, &name)),
                &state.members,
            );
        }

        // Recorded even when empty: a behavior that declares no fields still has
        // to be findable, or a reload would treat "no layout" and "no fields" as
        // the same thing and reset an instance that had nothing to lose.
        self.program.behaviors.push(layout.clone());
        self.collect_entries(&layout, decl);
        self.behavior = Some(layout);

        self.compile_field_defaults(decl);
        self.compile_state_entries(decl);

        for member in &decl.members {
            if let BehaviorMember::State(state) = member {
                self.compile_state(&decl.name, state);
                continue;
            }
            // Through the same pair `collect_signatures` used, so the name a
            // body is emitted under is by construction the name an index was
            // counted for. Fields fall out here, and a scheduled body is a
            // function of no arguments emitted below — what makes it scheduled
            // is the countdown the layout records, not anything about the code.
            let Some((name, _, _)) = member_signature(&decl.name, None, member) else {
                continue;
            };
            let Some((params, body)) = member_body(member) else {
                continue;
            };
            self.compile_body(&name, params, body);
        }

        // Emitted after the members, in the order `collect_signatures` counted.
        for (index, name) in sites::timer_names(&decl.name, &decl.members) {
            let body = match &decl.members[index] {
                BehaviorMember::Every(every) => &every.body,
                BehaviorMember::After(after) => &after.body,
                _ => continue,
            };
            self.compile_body(&name, &[], body);
        }

        self.fields.clear();
        self.behavior = None;
    }

    /// Compiles a state's members, with its own data in scope alongside the
    /// behavior's.
    ///
    /// A state's member is a function like any other, named
    /// `Guard.Patrol.Update` so dispatch can prefer it over the behavior's own.
    /// What makes it a *state's* member is only which slots it can name — which
    /// is the containment `state` exists to give.
    fn compile_state(&mut self, behavior: &str, decl: &crate::ast::StateDecl) {
        let Some(layout) = self.behavior.clone() else {
            return;
        };
        let Some(state) = layout
            .state_index(&decl.name)
            .and_then(|i| layout.state_at(i))
        else {
            return;
        };

        // Added on top of the behavior's fields rather than replacing them: a
        // state can read `health` while patrolling, and only the patrol's own
        // data is confined.
        let outer = self.fields.clone();
        self.state = Some(decl.name.clone());
        for (offset, slot) in state.slots.iter().enumerate() {
            let absolute = (layout.state_data_slot() + offset) as u16;
            let shape = shape_of_state_slot(decl, slot);
            self.fields.insert(slot.clone(), (absolute, shape));
        }

        // A state inside a state is not in the grammar, and a field is not a
        // function — both fall out of the pair below rather than needing an arm.
        for member in &decl.members {
            let Some((name, _, _)) = member_signature(behavior, Some(&decl.name), member) else {
                continue;
            };
            let Some((params, body)) = member_body(member) else {
                continue;
            };
            self.compile_body(&name, params, body);
        }

        // Its scheduled bodies last, in the order `collect_signatures` counted
        // them — the one contract that pair has, and the same one the
        // behavior's own timers keep.
        let owner = format!("{behavior}.{}", decl.name);
        for (index, name) in sites::timer_names(&owner, &decl.members) {
            let body = match &decl.members[index] {
                BehaviorMember::Every(every) => &every.body,
                BehaviorMember::After(after) => &after.body,
                _ => continue,
            };
            self.compile_body(&name, &[], body);
        }

        self.fields = outer;
        self.state = None;
    }

    /// Emits the function that gives a fresh instance its field values.
    ///
    /// A default is an *expression* — `float speed = 3.0;`, but also
    /// `Sqrt(2.0)` — so only compiled code can produce it. Without this a
    /// declared default would be decoration: the store would hand out unset
    /// slots and the first arithmetic would fault.
    ///
    /// A field with no written default gets its type's zero rather than staying
    /// unset, because `int health;` reads as a number that happens to start at
    /// nothing, not as a hole.
    /// Records what entering each of the behavior's states has to write.
    ///
    /// Read from the layout rather than recomputed, so the slots here are by
    /// construction the slots the running code reads.
    fn collect_entries(
        &mut self,
        layout: &crate::vm::BehaviorLayout,
        decl: &crate::ast::BehaviorDecl,
    ) {
        self.entries.clear();

        for member in &decl.members {
            let BehaviorMember::State(state) = member else {
                continue;
            };
            let Some(index) = layout.state_index(&state.name) else {
                continue;
            };

            // The parameters come first in the state's slots and are written by
            // `become` itself; only what the state *declares* needs a default.
            let base = layout.state_data_slot() + state.params.len();
            let fields = state
                .members
                .iter()
                .filter_map(|member| match member {
                    BehaviorMember::Field(field) => Some(field),
                    _ => None,
                })
                .enumerate()
                .map(|(offset, field)| {
                    (
                        (base + offset) as u16,
                        field.name.clone(),
                        field.default.clone(),
                        field.ty.clone(),
                    )
                })
                .collect();
            let params = state
                .params
                .iter()
                .map(|param| (param.name.clone(), param.ty.clone()))
                .collect();
            let slots = layout
                .state_at(index)
                .map(|entered| {
                    entered
                        .slots
                        .iter()
                        .enumerate()
                        .map(|(offset, slot)| {
                            let absolute = (layout.state_data_slot() + offset) as u16;
                            (slot.clone(), absolute, shape_of_state_slot(state, slot))
                        })
                        .collect()
                })
                .unwrap_or_default();

            let timers = layout
                .timers
                .iter()
                .enumerate()
                .filter(|(_, timer)| timer.state == Some(index))
                .map(|(slot, timer)| (layout.timer_slot(slot) as u16, timer.seconds))
                .collect();

            self.entries.insert(
                state.name.clone(),
                StateEntry {
                    params,
                    slots,
                    fields,
                    timers,
                },
            );
        }
    }

    /// Records every `every` and `after` among `members` in the layout.
    ///
    /// One walk for both levels, because a schedule is the same thing wherever
    /// it is written — what differs is only whose lifetime it follows, which is
    /// what `state` carries.
    fn collect_timers(
        &mut self,
        layout: &mut crate::vm::BehaviorLayout,
        behavior: &str,
        state: Option<(usize, &str)>,
        members: &[BehaviorMember],
    ) {
        let owner = match state {
            Some((_, name)) => format!("{behavior}.{name}"),
            None => behavior.to_owned(),
        };
        for (index, member_name) in sites::timer_names(&owner, members) {
            let (kind, interval) = match &members[index] {
                BehaviorMember::Every(every) => (crate::vm::TimerKind::Every, &every.interval),
                BehaviorMember::After(after) => (crate::vm::TimerKind::After, &after.delay),
                _ => continue,
            };
            let Some(seconds) = sites::literal_seconds(interval) else {
                // A field-driven interval needs the expression evaluated per
                // instance, which the schedule cannot do before it decides
                // whether to fire. Refusing beats scheduling a guess.
                self.error(
                    "an `every` or `after` interval must be a literal duration for now",
                    interval.span(),
                );
                continue;
            };
            layout.timers.push(crate::vm::TimerLayout {
                kind,
                seconds,
                member: member_name,
                state: state.map(|(index, _)| index),
            });
        }
    }

    fn compile_field_defaults(&mut self, decl: &crate::ast::BehaviorDecl) {
        self.code = Vec::new();
        self.locals = Vec::new();
        self.registers = Registers::new(0);
        self.naming.reset();
        self.record_entry();
        // An initialiser has no statements: each default is a step of its own,
        // named by the field, so a call it makes has a return site to name.
        self.naming.open_block();

        for member in &decl.members {
            let BehaviorMember::Field(field) = member else {
                continue;
            };
            let Some((slot, _)) = self.fields.get(&field.name).copied() else {
                continue;
            };

            self.naming.enter_step(&format!(
                "let.{}",
                keys::field_key(&field.name, field.default.as_ref())
            ));
            let mark = self.registers.mark();
            let src = match &field.default {
                Some(expr) => self.compile_expr(expr).0,
                None => self.zero_of_type(&field.ty),
            };
            self.emit(Instruction::StoreField { slot, src });
            self.registers.release_to(mark);
            self.naming.leave();
        }

        // A behavior with states starts in the one declared first. Left unset,
        // an instance would be in *no* state, and every handler written inside
        // one would silently never fire — a state machine that compiles and
        // does nothing.
        if let Some(layout) = self.behavior.clone() {
            if !layout.states.is_empty() {
                let slot = layout.state_slot() as u16;
                let mark = self.registers.mark();
                let (src, _) = self.constant(crate::vm::Value::Int(0), Shape::Int);
                self.emit(Instruction::StoreField { slot, src });
                self.registers.release_to(mark);
            }

            // Each countdown starts at its full interval, so `every 0.5s` first
            // fires half a second in rather than on the frame the entity
            // appeared — which is what "every half second" says.
            for (index, timer) in layout.timers.iter().enumerate() {
                let slot = layout.timer_slot(index) as u16;
                let mark = self.registers.mark();
                let (src, _) = self.constant(crate::vm::Value::Float(timer.seconds), Shape::Float);
                self.emit(Instruction::StoreField { slot, src });
                self.registers.release_to(mark);
            }

            // The first state is entered, not merely selected: its declared
            // data has to be there before anything reads it, exactly as a
            // `become` into it would leave things.
            if let Some(first) = layout.states.first().map(|state| state.name.clone()) {
                self.naming
                    .enter_step(&format!("become.{}", keys::state_key(&first)));
                self.emit_state_entry(&first, None);
                self.naming.leave();
            }
        }
        self.naming.close_block();

        let unit = self.registers.temp();
        self.emit(Instruction::LoadConst {
            dst: unit,
            value: crate::vm::Value::Unit,
        });
        self.emit(Instruction::Return { src: unit });

        let code = std::mem::take(&mut self.code);
        self.program.functions.push(Function {
            name: init_name(&decl.name),
            arity: 0,
            registers: self.registers.frame_size(),
            code,
            fingerprint: 0,
            sites: std::mem::take(&mut self.naming.sites),
        });
    }
}
