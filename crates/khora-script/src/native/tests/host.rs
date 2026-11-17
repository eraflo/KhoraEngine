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

//! The host's frame resources: the arena it is lent, and the reserver its spawns take ids from.

use std::sync::Arc;

use khora_core::ecs::EntityReserver;

use crate::arena::{Arena, Object};
use crate::native::Host;
use crate::vm::Value;

/// **A lent arena comes back with its objects**, and the host keeps an empty one.
#[test]
fn a_lent_arena_comes_back_with_its_objects() {
    let mut arena = Arena::new();
    let held = arena
        .alloc(Object::Array(vec![Value::Int(1)]))
        .expect("room in a fresh arena");
    let mut host = Host::bare().with_arena(arena);

    let back = host.take_arena();

    assert_eq!(back.get(held), Ok(&Object::Array(vec![Value::Int(1)])));
    assert_eq!(host.arena.len(), 0, "the host keeps an empty arena");
}

/// **A host reserves from the reserver it is given**: the world's, not one of its own.
#[test]
fn a_host_reserves_from_the_reserver_it_is_given() {
    let reserver = Arc::new(EntityReserver::new());

    let host = Host::bare().with_reserver(Arc::clone(&reserver));

    assert!(Arc::ptr_eq(&host.reserver, &reserver));
}
