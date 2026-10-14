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

//! What several test modules need alike.

use khora_script::arena::PersistentStore;
use khora_script::Program;

/// A fresh instance of `behavior`'s store, built as the engine builds one:
/// from the layout alone — every field at its default, the first state
/// entered with its data, every countdown armed. No code runs for it.
pub fn fresh_store(program: &Program, behavior: &str) -> PersistentStore {
    program
        .layout(behavior)
        .unwrap_or_else(|| panic!("`{behavior}` is a behavior of the program"))
        .fresh_store()
}
