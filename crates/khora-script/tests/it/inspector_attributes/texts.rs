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

//! The text of a tooltip and a header as the inspector receives it, and the
//! attributes of a field declared in an imported module.

use khora_script::{compile_module, MemoryLoader};

use super::{build, hints, HOSTS};

/// **A tooltip and a header are carried as written**: escapes resolved,
/// non-ASCII text and surrounding spaces kept, nothing trimmed or folded.
#[test]
fn a_tooltip_and_a_header_are_carried_as_written() {
    for (_, host, component) in HOSTS {
        let program = build(&host(
            "[Tooltip(\"  Dégâts \\\"forts\\\"\\n→ 攻撃  \")] [Header(\"Équipement\")] int hp = 1;",
        ));
        let hints = hints(&program, component, "hp");
        assert_eq!(
            hints.tooltip.as_deref(),
            Some("  Dégâts \"forts\"\n→ 攻撃  "),
            "{component}"
        );
        assert_eq!(hints.header.as_deref(), Some("Équipement"), "{component}");
        assert_eq!(hints.range, None, "{component}");
    }
}

/// **A field of an imported module carries its hints**, and one of its
/// refusals stops the program that imports it.
#[test]
fn a_field_declared_in_an_imported_module_carries_its_hints() {
    let loader = MemoryLoader::new()
        .with(
            "gear/shield.erg",
            "component Shield { [Range(0, 50)] [Tooltip(\"Absorbs\")] int armor = 10; }",
        )
        .with(
            "main.erg",
            "import \"gear/shield.erg\";
             behavior Guard { int hp = 1; void Update(float dt) { hp += 1; } }",
        );
    let result = compile_module(&loader, "main.erg");
    let program = result.program.expect("the imported module compiles");
    let shield = hints(&program, "Shield", "armor");
    assert_eq!(shield.tooltip.as_deref(), Some("Absorbs"));
    let range = shield.range.expect("`armor` carries its range");
    assert_eq!((range.min, range.max), (0.0, 50.0));

    let refused = MemoryLoader::new()
        .with(
            "gear/shield.erg",
            "component Shield { [Range(0, 50)] int armor = 99; }",
        )
        .with(
            "main.erg",
            "import \"gear/shield.erg\";\nbehavior Guard { int hp = 1; }",
        );
    let result = compile_module(&refused, "main.erg");
    assert!(
        !result.succeeded(),
        "a default outside its range in an imported module stops the program"
    );
}
