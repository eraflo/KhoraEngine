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

//! A query binding's type is `Entity`, a component or a component made
//! optional; any other is refused naming the type as written.

use khora_script::{compile_module, MemoryLoader, ENGINE_COMPONENTS_MODULE};

const ENGINE: &str = "
component Health { int current; int max; }
";

/// **A binding of an array type is refused naming that type** — never
/// `this`, which is another thing in the language, and in a system one the
/// checker refuses for itself.
#[test]
fn a_binding_of_an_array_type_is_refused_naming_its_type() {
    for main in [
        "behavior Census { void Update(float dt) { foreach (Health h, Health[] hs) { } } }",
        "system Sweep { void Update(float dt) { foreach (Health h, Health[] hs) { } } }",
    ] {
        let loader = MemoryLoader::new()
            .with(ENGINE_COMPONENTS_MODULE, ENGINE)
            .with(
                "main.erg",
                format!("import \"{ENGINE_COMPONENTS_MODULE}\";\n{main}"),
            );
        let result = compile_module(&loader, "main.erg");
        assert!(!result.succeeded(), "refused:\n{main}");
        let messages: Vec<&str> = result
            .diagnostics
            .iter()
            .filter(|d| d.is_error())
            .map(|d| d.message.as_str())
            .collect();
        assert!(
            messages.iter().any(|message| message.contains("Health[]")),
            "the refusal names `Health[]`: {messages:?}"
        );
        assert!(
            messages.iter().all(|message| !message.contains("`this`")),
            "no refusal speaks of `this`, which was never written: {messages:?}"
        );
    }
}
