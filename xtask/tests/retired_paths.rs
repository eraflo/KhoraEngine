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

//! Nothing still names a home the code has left: the module paths, source
//! paths and directories retired when concrete and editor code left
//! `khora-core`, GORNA moved under `agent`, and the hub joined `crates/`.

use std::fs;
use std::path::{Path, PathBuf};

/// Spellings of homes that no longer exist.
const RETIRED: &[&str] = &[
    // GORNA types and the PID left `khora_core::control`.
    "khora_core::control",
    "khora-core::control",
    "khora-core/src/control",
    "khora-core/control/",
    // Concrete code left khora-core.
    "khora_core::memory::SaaTrackingAllocator",
    "khora_core::audio::DefaultMixBus",
    "khora_core::audio::mix_bus::DefaultMixBus",
    "khora-core/src/memory/tracking_allocator",
    "khora_core::renderer::api::util::uniform_ring_buffer",
    "khora_core::renderer::api::util::dynamic_uniform_buffer",
    "khora-core/src/renderer/api/util/uniform_ring_buffer",
    "khora-core/src/renderer/api/util/dynamic_uniform_buffer",
    // Editor code left khora-core.
    "khora_core::ui::editor_overlay",
    "khora-core/src/ui/editor_overlay",
    "khora_core::ui::editor::dock",
    "khora_core::ui::editor::camera",
    "khora_core::ui::editor::command",
    "khora_core::ui::editor::gizmo_interact",
    "khora_core::ui::editor::log_capture",
    "khora-core/src/ui/editor/dock",
    "khora-core/src/ui/editor/camera",
    "khora-core/src/ui/editor/command",
    "khora-core/src/ui/editor/gizmo_interact",
    "khora-core/src/ui/editor/log_capture",
    "mod_gizmo",
    // The hub directory.
    "`hub/",
    "(hub/",
    "\"hub/",
];

/// A moved type and the crate that is its home now.
const MOVED_TYPES: &[(&str, &str)] = &[
    ("SaaTrackingAllocator", "khora-infra"),
    ("DefaultMixBus", "khora-infra"),
    ("EditorCamera", "khora-editor"),
    ("CommandHistory", "khora-editor"),
    ("EditorLogCapture", "khora-editor"),
    ("GizmoDrag", "khora-editor"),
    ("DockTree", "khora-tool-ui"),
    ("PidController", "khora-control"),
    ("UniformRingBuffer", "khora-lanes"),
];

#[test]
fn nothing_names_a_retired_path() {
    let root = repo_root();
    let mut hits = Vec::new();
    for file in scanned_files(&root) {
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        for (number, line) in text.lines().enumerate() {
            for retired in RETIRED {
                if line.contains(retired) {
                    hits.push(format!("{}:{}: `{retired}`", rel(&root, &file), number + 1));
                }
            }
        }
    }
    assert!(
        hits.is_empty(),
        "retired paths still named:\n  {}",
        hits.join("\n  ")
    );
}

/// A doc line that places a moved type in `khora-core` without naming the
/// crate it lives in now.
#[test]
fn docs_do_not_place_a_moved_type_in_khora_core() {
    let root = repo_root();
    let mut hits = Vec::new();
    for file in scanned_files(&root) {
        if file.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        for (number, line) in text.lines().enumerate() {
            let names_core = line.contains("khora-core") || line.contains("khora_core");
            if !names_core {
                continue;
            }
            for (ty, home) in MOVED_TYPES {
                let home_snake = home.replace('-', "_");
                if line.contains(ty) && !line.contains(home) && !line.contains(&home_snake) {
                    hits.push(format!(
                        "{}:{}: `{ty}` beside khora-core, home is {home}",
                        rel(&root, &file),
                        number + 1
                    ));
                }
            }
        }
    }
    assert!(
        hits.is_empty(),
        "moved types still placed in khora-core:\n  {}",
        hits.join("\n  ")
    );
}

/// Module paths, file names and type names the naming sweep retired.
const RENAMED: &[&str] = &[
    // khora-core
    "khora_core::utils",
    "khora-core/src/utils",
    "khora_core::context::",
    "khora_core::ui::app::context",
    "khora_core::lane::registry",
    "khora_core::runtime::registry",
    "khora_core::asset::uuid",
    "khora_core::renderer::api::command::encoder",
    "khora_core::renderer::api::command::compute",
    "khora_core::renderer::api::core",
    "khora-core/src/renderer/api/core",
    "khora_core::renderer::api::scene",
    "khora-core/src/renderer/api/scene",
    "khora_core::renderer::api::shader_defs",
    "khora_core::renderer::api::resource::shader_source",
    "khora_core::renderer::api::util::enums",
    "khora_core::renderer::forward_plus",
    "khora_sdk::renderer::scene",
    // khora-data
    "khora_data::ecs::registry",
    "khora_data::scene::registry",
    "khora_data::ecs::components::material_registry",
    "khora-data::ecs::components::material_registry",
    "khora_data::gpu::store",
    // khora-control
    "khora_control::registry",
    "khora_control::context",
    "khora_control::service",
    "khora-control/src/service/",
    // khora-telemetry
    "khora_telemetry::utils",
    "khora_telemetry::service",
    "khora_telemetry::metrics::registry",
    "khora_telemetry::monitoring::registry",
    // khora-io
    "khora_io::vfs",
    "khora-io::vfs",
    "khora-io/src/vfs",
    "VirtualFileSystem",
    "khora_io::asset::service",
    "khora_io::asset::registry",
    "khora_io::serialization::service",
    "khora_io::script_compile",
    "khora_io::script_mirror",
    "khora_io::script_hot_reload",
    "khora_io::asset_resolver",
    "khora_io::shader_hot_reload",
    "script_mirror.rs",
    "script_compile.rs",
    "script_hot_reload.rs",
    // khora-script, khora-infra, khora-lanes
    "khora_script::native::context",
    "khora_script::native::registry",
    "taffy_layout",
    "khora_lanes::render_lane::util::dynamic_uniform_buffer",
    // docs, xtask
    "open_questions",
    "xtask/src/helpers",
];

/// Nothing names a path, file or type the naming sweep retired — in the
/// Rust and Markdown files `scanned_files` covers, and also in the WGSL
/// comments that name their Rust mirror, the repository READMEs and the
/// tracked `.github` files.
#[test]
fn nothing_names_a_path_the_naming_sweep_retired() {
    let root = repo_root();
    let mut files = scanned_files(&root);
    let mut extra = Vec::new();
    if let Ok(entries) = fs::read_dir(root.join("crates")) {
        for entry in entries.flatten() {
            walk(&entry.path().join("src"), &mut extra);
        }
    }
    extra.retain(|p| p.extension().and_then(|e| e.to_str()) == Some("wgsl"));
    for dir in [".github/workflows", ".github/ISSUE_TEMPLATE"] {
        walk(&root.join(dir), &mut extra);
    }
    for file in [
        "README.md",
        "docs/README.md",
        ".agent/README.md",
        ".github/PULL_REQUEST_TEMPLATE.md",
    ] {
        extra.push(root.join(file));
    }
    files.extend(extra);
    let mut hits = Vec::new();
    for file in files {
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        for (number, line) in text.lines().enumerate() {
            for retired in RENAMED {
                if line.contains(retired) {
                    hits.push(format!("{}:{}: `{retired}`", rel(&root, &file), number + 1));
                }
            }
        }
    }
    assert!(
        hits.is_empty(),
        "names the naming sweep retired are still used:\n  {}",
        hits.join("\n  ")
    );
}

/// What scene records replaced: the four serialization strategies, the recipe
/// commands, the payload migrations and the raw memory dump of the world.
const RETIRED_SCENE_CODECS: &[&str] = &[
    "SerializationStrategy",
    "SceneRecipe",
    "SceneCommand",
    "SceneMigration",
    "migrate_payload",
    "serialize_recipe",
    "deserialize_recipe",
    "serialize_archetype",
    "deserialize_archetype",
    "DeserializeArchetypeError",
    "SceneMemoryLayout",
    "archetype_io",
    "khora_data::scene::strategy",
    "khora-data/src/scene/strategy",
    "KH_DEFINITION_RON_V1",
    "KH_ARCHETYPE_V1",
    "KH_MESSAGEPACK_V1",
];

/// The files that held them.
const RETIRED_SCENE_FILES: &[&str] = &[
    "crates/khora-data/src/scene/strategy",
    "crates/khora-data/src/scene/recipe.rs",
    "crates/khora-data/src/scene/migrations.rs",
    "crates/khora-data/src/ecs/world/archetype_io.rs",
    "crates/khora-data/src/ecs/serialization.rs",
];

/// The one place the pre-record codec may still live: the upgrade command
/// that reads old files once to rewrite them.
const LEGACY_SCENE_CODEC: &str = "xtask/src/commands/legacy_scene/";

/// Scenes are written as records now. The strategies, the recipe commands,
/// the payload migrations and the memory dump are gone — their files and
/// every mention of them — except in the upgrade command that reads old files.
#[test]
fn nothing_names_a_retired_scene_codec() {
    let root = repo_root();
    let mut hits = Vec::new();
    for retired in RETIRED_SCENE_FILES {
        if root.join(retired).exists() {
            hits.push(format!("{retired} still exists"));
        }
    }
    for file in scanned_files(&root) {
        if rel(&root, &file).starts_with(LEGACY_SCENE_CODEC) {
            continue;
        }
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        for (number, line) in text.lines().enumerate() {
            for retired in RETIRED_SCENE_CODECS {
                if line.contains(retired) {
                    hits.push(format!("{}:{}: `{retired}`", rel(&root, &file), number + 1));
                }
            }
        }
    }
    assert!(
        hits.is_empty(),
        "retired scene codecs still named:\n  {}",
        hits.join("\n  ")
    );
}

fn scanned_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for dir in [
        "docs/src",
        ".agent/engine",
        ".agent/gamedev",
        "examples",
        "xtask/src",
    ] {
        walk(&root.join(dir), &mut out);
    }
    if let Ok(entries) = fs::read_dir(root.join("crates")) {
        for entry in entries.flatten() {
            walk(&entry.path().join("src"), &mut out);
            walk(&entry.path().join("tests"), &mut out);
        }
    }
    out.retain(|path| {
        let r = rel(root, path);
        // Dated logs record history in the words of their day.
        !r.contains("/knowledge/")
            && r != "docs/src/roadmap.md"
            && matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("rs" | "md" | "toml")
            )
    });
    out.sort();
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().and_then(|n| n.to_str()) != Some("target") {
                walk(&path, out);
            }
        } else {
            out.push(path);
        }
    }
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| panic!("{} has no parent", env!("CARGO_MANIFEST_DIR")))
}
