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

//! **Where a clash with the engine's component mirror is located** (Ergon
//! spec 11): the mirror is served by the engine, never a file under the
//! script root, so a problem a tool lists there has no file to open.

use khora_core::script::ScriptProblems;
use khora_io::script::mirror::{PreludeLoader, MIRROR_MODULE};
use khora_io::script::problems::report_compile;
use khora_script::{compile_module, MemoryLoader};

/// A project struct named like an engine component, both imported by one
/// entry: the compile's problem is located in the mirror — a module a tool
/// cannot open as a file.
#[test]
fn a_clash_with_the_engine_mirror_is_located_in_the_mirror() {
    let main =
        format!("import \"helper.erg\";\nimport \"{MIRROR_MODULE}\";\nfn void Main() {{ }}\n");
    let loader = PreludeLoader::new(
        MemoryLoader::new()
            .with("helper.erg", "struct Transform { int x; }\n")
            .with("main.erg", &main),
    );
    let problems = ScriptProblems::default();

    report_compile(&problems, "main.erg", &compile_module(&loader, "main.erg"));

    let all = problems.all();
    assert!(!all.is_empty(), "the premise: a clash");
    assert!(
        all.iter()
            .any(|problem| problem.module.as_deref() == Some(MIRROR_MODULE)),
        "{all:?}"
    );
}
