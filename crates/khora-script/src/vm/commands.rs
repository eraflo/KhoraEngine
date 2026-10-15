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

//! Component writes — `Set`, `Add`, `Remove`, `Spawn` — and reads — `Get`,
//! `Has`.
//!
//! A script never touches the `World`: each write becomes a
//! [`WorldCommand`] queued in [`Host::commands`] — where every native's
//! command already goes — and the frame boundary applies it. A patch carries
//! the fields it names and nothing else, so what a script did not name is left
//! as the entity holds it.
//!
//! A read answers from the frame's working set (`Host::components`): the
//! projection of what the loaded programs read, with this frame's own writes
//! over it — every write is observed as it is queued.

use khora_core::math::Quaternion;
use khora_core::script::{ComponentName, ScriptValue, WorldCommand};

use super::instruction::WriteMode;
use super::{Fault, Instruction, Machine, Program, Reg, Step, Value};
use crate::native::Host;

impl Machine {
    /// Executes a component write.
    pub(super) fn step_command(
        &mut self,
        instruction: &Instruction,
        program: &Program,
        host: &mut Host,
    ) -> Result<Step, Fault> {
        match *instruction {
            Instruction::WriteComponent {
                mode,
                entity,
                patch,
                base,
                ..
            } => {
                let entity = self.entity_at(entity)?;
                let (component, value) = self.patch_value(program, host, patch, base)?;
                // Rows written earlier in the run go first.
                self.send_rows(host, Some(component.as_str()));
                let command = match mode {
                    WriteMode::Set => WorldCommand::SetComponent {
                        entity,
                        component,
                        value,
                    },
                    WriteMode::Add => WorldCommand::AddComponent {
                        entity,
                        component,
                        value,
                    },
                };
                // A script reads its own writes within the frame.
                host.components.observe(&command);
                host.commands.push(command);
            }
            Instruction::GetComponent {
                dst,
                entity,
                component,
                ..
            } => {
                let entity = self.entity_at(entity)?;
                let name = program.string(component).ok_or(Fault::BadString)?;
                let copy = match host.components.get(name, entity) {
                    None => Value::Null,
                    Some(read) => {
                        let unreadable = |reason: String| Fault::Unreadable {
                            component: name.to_owned(),
                            reason,
                        };
                        let owned = crate::bridge::to_owned_as(&read, name, &program.structs)
                            .map_err(|error| unreadable(error.to_string()))?;
                        host.arena
                            .import(&owned, program)
                            .map_err(|error| unreadable(format!("{error:?}")))?
                    }
                };
                self.write(dst, copy)?;
            }
            Instruction::HasComponent {
                dst,
                entity,
                component,
            } => {
                let entity = self.entity_at(entity)?;
                let name = program.string(component).ok_or(Fault::BadString)?;
                self.write(dst, Value::Bool(host.components.has(name, entity)))?;
            }
            Instruction::QueryRows { dst, query } => {
                let shape = program.query(query).ok_or(Fault::BadString)?;
                let all: Vec<&str> = shape.all.iter().map(String::as_str).collect();
                let none: Vec<&str> = shape.none.iter().map(String::as_str).collect();
                let rows = host
                    .components
                    .matching(&all, &none)
                    .into_iter()
                    .map(Value::Entity)
                    .collect::<Vec<_>>();
                host.swept += rows.len();
                let handle = host
                    .arena
                    .alloc(crate::arena::Object::Array(rows))
                    .map_err(|_| Fault::ArenaFull)?;
                self.write(dst, Value::Obj(super::ObjRef::Arena(handle)))?;
            }
            Instruction::QueryHolds {
                dst, entity, query, ..
            } => {
                let entity = self.entity_at(entity)?;
                let shape = program.query(query).ok_or(Fault::BadString)?;
                let holds = shape
                    .all
                    .iter()
                    .all(|name| host.components.has(name, entity))
                    && !shape
                        .none
                        .iter()
                        .any(|name| host.components.has(name, entity));
                self.write(dst, Value::Bool(holds))?;
            }
            Instruction::RemoveComponent { entity, component } => {
                let entity = self.entity_at(entity)?;
                let name = program.string(component).ok_or(Fault::BadString)?;
                let command = WorldCommand::RemoveComponent {
                    entity,
                    component: ComponentName::new(name),
                };
                self.send_rows(host, Some(name));
                host.components.observe(&command);
                host.commands.push(command);
            }
            Instruction::Raise {
                entity,
                event,
                base,
                argc,
            } => {
                let target = self.entity_at(entity)?;
                let name = program.string(event).ok_or(Fault::BadString)?;
                let mut raised = khora_core::script::ScriptEvent::new(target, name);
                for offset in 0..Reg::from(argc) {
                    let value = self.read(base + offset)?;
                    // Through the one bridge, so what an event carries is
                    // exactly what a handler can receive.
                    let carried =
                        crate::bridge::from_register(value, &program.strings, &host.arena)
                            .map_err(|why| Fault::Unraisable {
                                event: name.to_owned(),
                                why: why.to_string(),
                            })?;
                    raised = raised.with(carried);
                }
                host.outbox.push(raised);
            }
            Instruction::SpawnEntity {
                position,
                spawn,
                base,
                ..
            } => {
                let at = match self.read(position)? {
                    Value::Vec3(at) => at,
                    other => return Err(mismatch("Vec3", other)),
                };
                let patches = program.spawns.get(spawn as usize).ok_or(Fault::BadString)?;
                let mut components = Vec::with_capacity(patches.len());
                let mut next = base;
                for &patch in patches {
                    let width = program
                        .patches
                        .get(patch as usize)
                        .map_or(0, |patch| patch.fields.len());
                    components.push(self.patch_value(program, host, patch, next)?);
                    next = next.wrapping_add(width as Reg);
                }
                // Upright: the new entity turns itself in its own `OnSpawn`
                // when it should face elsewhere.
                host.commands.push(WorldCommand::Spawn {
                    position: at,
                    rotation: Quaternion::IDENTITY,
                    components,
                });
            }
            _ => unreachable!("step_command is reached only for component writes"),
        }
        Ok(Step::Next)
    }

    /// The entity in `register`.
    pub(super) fn entity_at(
        &self,
        register: Reg,
    ) -> Result<khora_core::ecs::entity::EntityId, Fault> {
        match self.read(register)? {
            Value::Entity(entity) => Ok(entity),
            other => Err(mismatch("Entity", other)),
        }
    }

    /// Patch `patch`'s component and its fields' values, read from `base..`,
    /// as the boundary carries them.
    fn patch_value(
        &self,
        program: &Program,
        host: &Host,
        patch: u32,
        base: Reg,
    ) -> Result<(ComponentName, ScriptValue), Fault> {
        let declared = program
            .patches
            .get(patch as usize)
            .ok_or(Fault::BadString)?;
        let mut fields = Vec::with_capacity(declared.fields.len());
        for (offset, field) in declared.fields.iter().enumerate() {
            let value = self.read(base.wrapping_add(offset as Reg))?;
            let owned = host
                .arena
                .export(value, program)
                .map_err(|_| Fault::BadString)?;
            let boundary =
                crate::bridge::from_owned(&owned).map_err(|error| Fault::Unwritable {
                    component: declared.component.clone(),
                    field: field.clone(),
                    reason: error.to_string(),
                })?;
            fields.push((field.clone(), boundary));
        }
        Ok((
            ComponentName::new(declared.component.as_str()),
            ScriptValue::Struct(fields),
        ))
    }
}

fn mismatch(expected: &'static str, found: Value) -> Fault {
    Fault::TypeMismatch {
        expected,
        found: found.type_name(),
    }
}
