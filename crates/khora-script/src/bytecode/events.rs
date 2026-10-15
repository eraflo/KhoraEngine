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

//! The events a program knows: the engine's and the ones its modules
//! declare — listed for the delivery, shaped for the raises, and signed for
//! the keys of the statements raising them.

use crate::ast::{Item, Module};

use super::{shape_of, Compiler, Shape};

impl Compiler {
    /// The events the module declares — its own, its imports', the engine's:
    /// their shapes for the raises, their signatures for the program.
    pub(super) fn collect_events(&mut self, module: &Module) {
        for event in khora_core::script::ENGINE_EVENTS {
            let types: Vec<khora_core::script::ErgonType> = event
                .params
                .iter()
                .map(|(ty, _)| {
                    khora_core::script::ErgonType::parse(ty)
                        .unwrap_or(khora_core::script::ErgonType::Any)
                })
                .collect();
            self.event_shapes.insert(
                event.name.to_owned(),
                types
                    .iter()
                    .map(|ty| match ty {
                        khora_core::script::ErgonType::Int => Shape::Int,
                        khora_core::script::ErgonType::Float
                        | khora_core::script::ErgonType::Duration
                        | khora_core::script::ErgonType::Angle => Shape::Float,
                        _ => Shape::Other,
                    })
                    .collect(),
            );
            self.program.events.push(crate::vm::EventSignature {
                name: event.name.to_owned(),
                params: types,
            });
        }
        for item in &module.items {
            let Item::Event(decl) = item else {
                continue;
            };
            self.event_shapes.insert(
                decl.name.clone(),
                decl.params
                    .iter()
                    .map(|param| shape_of(&param.ty))
                    .collect(),
            );
            self.program.events.push(crate::vm::EventSignature {
                name: decl.name.clone(),
                params: decl
                    .params
                    .iter()
                    .map(|param| {
                        crate::modules::components::ergon_type(&param.ty)
                            .unwrap_or(khora_core::script::ErgonType::Any)
                    })
                    .collect(),
            });
        }
        self.program.events.sort_by(|a, b| a.name.cmp(&b.name));
        self.naming.events = self
            .program
            .events
            .iter()
            .map(|event| {
                let params: Vec<String> = event.params.iter().map(|ty| ty.to_string()).collect();
                (event.name.clone(), params.join(","))
            })
            .collect();
    }
}
