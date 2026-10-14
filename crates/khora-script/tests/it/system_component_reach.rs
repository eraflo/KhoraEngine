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

//! A system's component is held by its one entity, and by no other: no
//! script attaches it, writes it, or sweeps it — an entity given it would be
//! a second instance of the system, run each frame beside the first.

use khora_script::{compile_module, MemoryLoader, ENGINE_COMPONENTS_MODULE};

const ENGINE: &str = "component Health { int current; int max; }";

fn compiles(main: &str) -> bool {
    let loader = MemoryLoader::new()
        .with(ENGINE_COMPONENTS_MODULE, ENGINE)
        .with(
            "main.erg",
            format!("import \"{ENGINE_COMPONENTS_MODULE}\";\n{main}"),
        );
    compile_module(&loader, "main.erg").succeeded()
}

const CLOCK: &str = "system Clock { int ticks = 0; void Update(float dt) { ticks += 1; } }";

/// **No script reaches a system's component through an entity**: `Set`,
/// `Add`, `Get`, `Has`, `Remove` of `Clock` on some entity, and a sweep
/// binding it, are each refused.
#[test]
fn no_script_attaches_writes_or_sweeps_a_systems_component() {
    assert!(compiles(&format!(
        "{CLOCK} behavior Probe {{ void Update(float dt) {{ Entity e = this; e.Set(Health {{ current: 1 }}); bool b = e.Has(Health); foreach (Health h) {{ }} }} }}"
    )));
    for body in [
        "Entity e = this; e.Set(Clock { ticks: 5 });",
        "Entity e = this; e.Add(Clock { ticks: 5 });",
        "Entity e = this; var c = e.Get(Clock);",
        "Entity e = this; bool b = e.Has(Clock);",
        "Entity e = this; e.Remove(Clock);",
        "foreach (Clock c) { }",
    ] {
        assert!(
            !compiles(&format!(
                "{CLOCK} behavior Probe {{ void Update(float dt) {{ {body} }} }}"
            )),
            "compiled: {body}"
        );
    }
}
