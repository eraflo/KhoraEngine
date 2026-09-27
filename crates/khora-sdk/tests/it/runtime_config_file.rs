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

//! `RuntimeConfig::load_or_default` reads `runtime.json` from the directory it
//! is given, and falls back to the defaults when the file is missing or
//! malformed rather than refusing to start.

use khora_sdk::{RuntimeConfig, RUNTIME_CONFIG_FILE};

fn scratch_dir(tag: &str) -> std::io::Result<std::path::PathBuf> {
    let dir = std::env::temp_dir().join(format!("khora-sdk-it-{tag}-{}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[test]
fn a_runtime_json_on_disk_is_read_and_a_bad_one_falls_back() -> std::io::Result<()> {
    let dir = scratch_dir("runtime-config")?;

    let missing = RuntimeConfig::load_or_default(&dir);

    std::fs::write(
        dir.join(RUNTIME_CONFIG_FILE),
        include_str!("../fixtures/runtime.json"),
    )?;
    let written = RuntimeConfig::load_or_default(&dir);

    std::fs::write(dir.join(RUNTIME_CONFIG_FILE), "{ not json")?;
    let malformed = RuntimeConfig::load_or_default(&dir);
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(missing, RuntimeConfig::defaults());
    assert_eq!(malformed, RuntimeConfig::defaults());
    assert_eq!(written.project_name, "Khora Demo");
    assert_eq!(written.window_title(), "Khora Demo");
    assert_eq!(written.preset.as_deref(), Some("release"));
    assert!(written.verify_integrity);
    Ok(())
}
