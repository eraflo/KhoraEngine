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

#[test]
fn host_target_matches_cfg() {
    let host = BuildTarget::host();
    if cfg!(target_os = "windows") {
        assert_eq!(host, BuildTarget::Windows);
    } else if cfg!(target_os = "macos") {
        assert_eq!(host, BuildTarget::Macos);
    } else {
        assert_eq!(host, BuildTarget::Linux);
    }
}

#[test]
fn target_suffixes_are_correct() {
    assert_eq!(BuildTarget::Windows.exe_suffix(), ".exe");
    assert_eq!(BuildTarget::Linux.exe_suffix(), "");
    assert_eq!(BuildTarget::Macos.exe_suffix(), "");
}

#[test]
fn sanitize_binary_name_strips_unsafe_chars() {
    assert_eq!(sanitize_binary_name("My Game!"), "My_Game_");
    assert_eq!(sanitize_binary_name("ok-name_1"), "ok-name_1");
    assert_eq!(sanitize_binary_name(""), "game");
    assert_eq!(sanitize_binary_name("/etc/passwd"), "_etc_passwd");
}

/// The `runtime.json` the runtime reads, for a project named "Khora Demo"
/// built with the Release preset. The SDK's reader test parses the same
/// file, so what is written here is what the runtime gets back.
const READ_BY_THE_RUNTIME: &str = include_str!("../../../khora-sdk/tests/fixtures/runtime.json");

/// Every field the runtime reads is written, with the value the runtime
/// expects — the window title included.
#[test]
fn the_runtime_json_the_editor_writes_is_what_the_runtime_reads() -> Result<()> {
    let dir =
        std::env::temp_dir().join(format!("khora-editor-runtime-json-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let written = write_runtime_config(&dir, "Khora Demo", BuildPreset::Release)
        .and_then(|()| Ok(std::fs::read_to_string(dir.join("runtime.json"))?));
    let _ = std::fs::remove_dir_all(&dir);

    let written: serde_json::Value = serde_json::from_str(&written?)?;
    let expected: serde_json::Value = serde_json::from_str(READ_BY_THE_RUNTIME)?;
    assert_eq!(written, expected);
    Ok(())
}

/// **A project's physics step reaches the built game.** The editor writes
/// the project's `physics` section into `runtime.json`, and the runtime
/// reads that step back; a project at the default step writes none.
#[test]
fn the_runtime_json_carries_the_projects_physics_step_to_the_runtime() -> Result<()> {
    let a_128th = PhysicsSettings {
        step: std::time::Duration::from_nanos(7_812_500),
    };
    let dir = std::env::temp_dir().join(format!(
        "khora-editor-runtime-json-physics-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir)?;
    let written = write_runtime_config_with(&dir, "Khora Demo", BuildPreset::Release, a_128th)
        .and_then(|()| Ok(std::fs::read_to_string(dir.join("runtime.json"))?));
    let _ = std::fs::remove_dir_all(&dir);

    let read: RuntimeConfig = serde_json::from_str(&written?)?;
    assert_eq!(
        read.physics, a_128th,
        "the runtime reads the project's step"
    );
    Ok(())
}

/// **Build Game writes the project's version.** The `runtime.json` a
/// game is built with carries the project's version as its
/// `game_version`, which the runtime reads back as the game's identity;
/// the rest is what Build Game always wrote — the name as project name
/// and window title, the preset, the physics step.
#[test]
fn build_game_writes_the_projects_version() -> Result<()> {
    let game = khora_sdk::GameIdentity {
        name: "Khora Demo".to_owned(),
        version: "2.3.4".to_owned(),
    };
    let a_128th = PhysicsSettings {
        step: std::time::Duration::from_nanos(7_812_500),
    };

    let cfg = runtime_config(&game, BuildPreset::Release, a_128th);

    assert_eq!(cfg.game_version, "2.3.4", "the project's version");
    assert_eq!(cfg.project_name, "Khora Demo");
    assert_eq!(cfg.window_title.as_deref(), Some("Khora Demo"));
    assert_eq!(cfg.default_scene, DEFAULT_SCENE_REL_PATH);
    assert_eq!(cfg.preset.as_deref(), Some(BuildPreset::Release.label()));
    assert_eq!(cfg.physics, a_128th);

    let read: RuntimeConfig = serde_json::from_str(&serde_json::to_string_pretty(&cfg)?)?;
    assert_eq!(
        read.game_identity(),
        game,
        "the runtime reads the game back from what Build Game writes"
    );
    Ok(())
}

/// The key in the one key slot `binary` holds, `None` when it holds no
/// slot or several.
fn key_in_slot(binary: &[u8]) -> Option<khora_sdk::PackKey> {
    let magic = khora_sdk::slot_magic();
    let mut slots = (0..binary.len().saturating_sub(64))
        .filter(|&at| binary[at] == magic[0] && binary[at..at + 32] == magic);
    let at = slots.next()?;
    if slots.next().is_some() {
        return None;
    }
    let key: [u8; 32] = binary[at + 32..at + 64].try_into().ok()?;
    Some(khora_sdk::PackKey(key))
}

/// **The build signs the pack and stamps the binary**, whatever the
/// preset: over a fake runtime binary holding an empty key slot, a build
/// signed by a key made in a temporary folder stages a binary whose slot
/// holds that key, and the `manifest.bin` beside it verifies with the key
/// read from the staged binary over the `index.bin` beside it, recording
/// every packed file. The runtime binary built from is left unstamped.
#[test]
fn the_build_signs_the_pack_and_stamps_the_binary() -> Result<()> {
    use khora_sdk::khora_core::asset::AssetUUID;

    const FILES: &[(&str, &[u8])] = &[
        ("scenes/default.kscene", b"the default scene"),
        ("textures/a.png", b"a texture"),
    ];
    let root =
        std::env::temp_dir().join(format!("khora-editor-signed-build-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let assets = root.join("project").join("assets");
    for (rel, bytes) in FILES {
        let path = assets.join(rel);
        std::fs::create_dir_all(path.parent().expect("a folder"))?;
        std::fs::write(path, bytes)?;
    }
    let runtime_src = root.join(format!("khora-runtime{}", BuildTarget::host().exe_suffix()));
    let mut fake_runtime = b"a fake runtime's code, then its slot: ".to_vec();
    fake_runtime.extend_from_slice(&khora_sdk::slot_magic());
    fake_runtime.extend_from_slice(&[0; 32]);
    fake_runtime.extend_from_slice(b" and the rest of the fake runtime");
    std::fs::write(&runtime_src, &fake_runtime)?;

    let outcome = (|| -> Result<Vec<(BuildPreset, BuildOutcome, khora_sdk::PackKey)>> {
        let signer = PackSigner::load_or_create(&root.join("signing"))?;
        let mut built = Vec::new();
        for preset in [
            BuildPreset::Debug,
            BuildPreset::Release,
            BuildPreset::Shipping,
        ] {
            let output_dir = root.join("dist").join(preset.label());
            let outcome = build_with_runtime(
                &assets,
                &output_dir,
                "Khora Demo",
                BuildTarget::host(),
                preset,
                &signer,
                &runtime_src,
            )?;
            built.push((preset, outcome, signer.public_key()));
        }
        Ok(built)
    })();
    let checked = outcome.and_then(|built| {
        for (preset, outcome, signer_key) in built {
            let staged = std::fs::read(&outcome.binary_path)?;
            let key = key_in_slot(&staged)
                .ok_or_else(|| anyhow!("{preset:?}: the staged binary holds no one slot"))?;
            assert_eq!(
                key, signer_key,
                "{preset:?}: the slot holds the signer's key"
            );
            assert_eq!(
                outcome.binary_path.file_name().and_then(|n| n.to_str()),
                Some(format!("Khora_Demo{}", BuildTarget::host().exe_suffix()).as_str()),
                "{preset:?}: the staged binary is named after the project"
            );

            let manifest_bin = std::fs::read(outcome.output_dir.join("manifest.bin"))?;
            let index_bin = std::fs::read(outcome.output_dir.join("index.bin"))?;
            let manifest = khora_sdk::SignedManifest::decode(&manifest_bin)
                .map_err(|e| anyhow!("{preset:?}: manifest.bin is not signed: {e}"))?
                .verify(&key, &index_bin)
                .map_err(|e| anyhow!("{preset:?}: manifest.bin does not verify: {e}"))?;
            assert_eq!(manifest.len(), FILES.len(), "{preset:?}");
            for (rel, bytes) in FILES {
                manifest
                    .verify(&AssetUUID::new_v5(rel), bytes)
                    .map_err(|e| anyhow!("{preset:?}: {rel} is not recorded: {e:#}"))?;
            }
        }
        Ok(std::fs::read(&runtime_src)?)
    });
    let _ = std::fs::remove_dir_all(&root);

    assert_eq!(
        checked?, fake_runtime,
        "the runtime binary built from was changed"
    );
    Ok(())
}
