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

//! Every script the repository ships compiles through the resolve → check →
//! compile pipeline, with nothing to report.

use std::path::{Path, PathBuf};

use khora_io::script_compile::DiskLoader;
use khora_script::compile_module;

/// Directory of the sandbox example's scripts.
fn sandbox_scripts() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/sandbox/assets/scripts")
}

#[test]
fn every_shipped_script_compiles_without_a_diagnostic() -> std::io::Result<()> {
    let root = sandbox_scripts();
    let mut entries: Vec<String> = std::fs::read_dir(&root)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".erg"))
        .collect();
    entries.sort();
    assert!(!entries.is_empty(), "no script under {}", root.display());

    let loader = DiskLoader::new(&root);
    for entry in &entries {
        let result = compile_module(&loader, entry);
        assert!(
            result.diagnostics.is_empty(),
            "{entry}: {:?}",
            result.diagnostics
        );
        assert!(result.program.is_some(), "{entry}: no program");
    }
    Ok(())
}

/// The hover behaviour the sandbox attaches to an entity is in the program.
#[test]
fn the_sandbox_hover_script_declares_its_behaviour() {
    let result = compile_module(&DiskLoader::new(sandbox_scripts()), "hover.erg");
    let program = result.program.expect("hover.erg compiles");
    assert!(program.layout("Hover").is_some());
}
