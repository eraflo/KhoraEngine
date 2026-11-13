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

use super::*;
use std::any::TypeId;

struct InServices(u8);
struct InBackends(u8);
struct Nowhere;

#[test]
fn look_up_reaches_every_container() {
    let mut runtime = Runtime::new();
    runtime.services.insert(InServices(1));
    runtime.backends.insert(InBackends(2));

    assert_eq!(runtime.look_up::<InServices>().map(|e| e.0), Some(1));
    assert_eq!(runtime.look_up::<InBackends>().map(|e| e.0), Some(2));
    assert!(runtime.look_up::<Nowhere>().is_none());
}

#[test]
fn holds_names_a_type_by_its_id() {
    let mut runtime = Runtime::new();
    runtime.backends.insert(InBackends(0));

    assert!(runtime.holds(TypeId::of::<InBackends>()));
    assert!(!runtime.holds(TypeId::of::<Nowhere>()));
}
