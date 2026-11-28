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

#[test]
fn a_project_named_like_a_windows_device_builds_a_binary_kept_apart_from_it() {
    assert_eq!(sanitize_binary_name("con"), "con_");
    assert_eq!(sanitize_binary_name("Nul"), "Nul_");
    assert_eq!(sanitize_binary_name("console"), "console");
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

/// **A project's physics step reaches the built game.**
///
/// The editor writes the project's `physics` section into `runtime.json`, and
/// the runtime reads that step back; a project at the default step writes none.
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

/// **Build Game writes the project's version.**
///
/// The `runtime.json` a game is built with carries the project's version as its
/// `game_version`, which the runtime reads back as the game's identity; the
/// rest is what Build Game always wrote — the name as project name and window
/// title, the preset, the physics step.
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
            let outcome = build_with_runtime(RuntimeStampBuild {
                assets_root: &assets,
                refusals: khora_sdk::trust::Refusals::detached(),
                output_dir: &output_dir,
                project_name: "Khora Demo",
                target: BuildTarget::host(),
                preset,
                signer: &signer,
                runtime_src: &runtime_src,
            })?;
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

/// A project under the temp folder, removed when dropped.
struct TempProject(std::path::PathBuf);

impl TempProject {
    /// Makes a project tagged `tag` holding `files`, by path under the root and bytes.
    fn with(tag: &str, files: &[(&str, &[u8])]) -> Result<Self> {
        let root =
            std::env::temp_dir().join(format!("khora-editor-build-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (rel, bytes) in files {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().expect("a folder"))?;
            std::fs::write(path, bytes)?;
        }
        std::fs::create_dir_all(root.join("assets"))?;
        Ok(Self(root))
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Returns the names of the folders in `dir`, sorted: the build's lock files beside them left out.
fn folders_in(dir: &std::path::Path) -> Result<Vec<String>> {
    let mut folders = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            folders.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    folders.sort();
    Ok(folders)
}

/// **A build says what it left out**: one external file missing is named in
/// the outcome and in the result line, which says it built and how many files
/// it left out; a build that left nothing out says it built.
#[test]
fn a_build_says_what_it_left_out() -> Result<()> {
    let project = TempProject::with(
        "left-out",
        &[(
            "assets/models/a.gltf",
            br#"{"asset":{"version":"2.0"},"buffers":[{"uri":"gone.bin","byteLength":4}]}"#,
        )],
    )?;
    let runtime_src = project
        .0
        .join(format!("khora-runtime{}", BuildTarget::host().exe_suffix()));
    let mut fake_runtime = b"a fake runtime, then its slot: ".to_vec();
    fake_runtime.extend_from_slice(&khora_sdk::slot_magic());
    fake_runtime.extend_from_slice(&[0; 32]);
    std::fs::write(&runtime_src, &fake_runtime)?;
    let signer = PackSigner::load_or_create(&project.0.join("signing"))?;

    let outcome = build_with_runtime(RuntimeStampBuild {
        assets_root: &project.0.join("assets"),
        refusals: khora_sdk::trust::Refusals::detached(),
        output_dir: &project.0.join("dist"),
        project_name: "Lantern",
        target: BuildTarget::host(),
        preset: BuildPreset::Debug,
        signer: &signer,
        runtime_src: &runtime_src,
    })?;

    assert_eq!(outcome.left_out.len(), 1, "{:?}", outcome.left_out);
    assert_eq!(outcome.left_out[0].path, "models/gone.bin");
    let line = result_line(&Ok(outcome.clone()));
    assert!(line.contains("built, 1 file left out"), "{line}");
    assert!(line.contains("models/gone.bin is missing"), "{line}");
    let clean = BuildOutcome {
        left_out: Vec::new(),
        ..outcome
    };
    let line = result_line(&Ok(clean));
    assert!(line.starts_with("Build Game: built via"), "{line}");
    Ok(())
}

/// **A failed build leaves the previous build whole**, removes its temporary
/// folder, and its result line names the asset refused and why.
#[test]
fn a_failed_build_leaves_the_previous_build_whole() -> Result<()> {
    let project = TempProject::with(
        "failed",
        &[
            ("Cargo.toml", b"[package]\nname = \"lantern\"\n"),
            ("dist/windows/previous.txt", b"the previous build"),
            ("dist/linux/previous.txt", b"the previous build"),
            ("dist/macos/previous.txt", b"the previous build"),
        ],
    )?;
    // One byte over a trusted project's cap of a text file: the pack refuses it.
    std::fs::create_dir_all(project.0.join("assets").join("materials"))?;
    std::fs::File::create(project.0.join("assets").join("materials").join("big.kmat"))?
        .set_len((64 << 20) + 1)?;
    let pvfs = crate::project_vfs::ProjectVfs::open(
        project.0.clone(),
        Arc::new(khora_sdk::MetricsRegistry::new()),
    )?;
    let signer = PackSigner::load_or_create(&project.0.join("signing"))?;

    let built = build_for_target(
        &pvfs,
        "Lantern",
        BuildTarget::host(),
        BuildPreset::Debug,
        &signer,
    );

    let line = result_line(&built);
    assert!(built.is_err(), "a refused asset fails the build");
    assert!(line.contains("big.kmat"), "{line}");
    assert!(line.contains("over the"), "the refusal's reason: {line}");
    let dist = project.0.join("dist");
    let target = dist.join(BuildTarget::host().dir_name());
    assert_eq!(
        std::fs::read(target.join("previous.txt"))?,
        b"the previous build"
    );
    assert_eq!(
        std::fs::read_dir(&target)?.count(),
        1,
        "nothing of the failed build is left"
    );
    assert_eq!(
        folders_in(&dist)?,
        ["linux", "macos", "windows"],
        "no temporary folder is left"
    );
    Ok(())
}

/// **A build puts back the build an interrupted one moved aside, and says so.**
///
/// A build killed between its two renames left no folder at the target and
/// the previous build moved aside. The next build, failing on a refused
/// asset, still leaves that previous build at the target, and its error says
/// it was put back.
#[test]
fn a_build_puts_back_the_build_an_interrupted_one_moved_aside() -> Result<()> {
    let dir = BuildTarget::host().dir_name();
    let moved_aside = format!("dist/{dir}.4242.7.previous.tmp/previous.txt");
    let project = TempProject::with(
        "interrupted",
        &[
            ("Cargo.toml", b"[package]\nname = \"lantern\"\n"),
            (moved_aside.as_str(), b"the previous build"),
        ],
    )?;
    std::fs::create_dir_all(project.0.join("assets").join("materials"))?;
    std::fs::File::create(project.0.join("assets").join("materials").join("big.kmat"))?
        .set_len((64 << 20) + 1)?;
    let pvfs = crate::project_vfs::ProjectVfs::open(
        project.0.clone(),
        Arc::new(khora_sdk::MetricsRegistry::new()),
    )?;
    let signer = PackSigner::load_or_create(&project.0.join("signing"))?;

    let built = build_for_target(
        &pvfs,
        "Lantern",
        BuildTarget::host(),
        BuildPreset::Debug,
        &signer,
    );

    let line = result_line(&built);
    assert!(built.is_err(), "a refused asset fails the build");
    assert!(line.contains("was put back"), "{line}");
    let dist = project.0.join("dist");
    assert_eq!(
        std::fs::read(dist.join(dir).join("previous.txt"))?,
        b"the previous build"
    );
    assert_eq!(
        folders_in(&dist)?,
        [dir],
        "nothing an interrupted build left remains"
    );
    Ok(())
}

/// **One build of a target at a time: a second is refused while the first holds it.**
///
/// The first build holds `dist/<target>` and fills its temporary folder. A
/// second build of that target is refused, says another build holds it, and
/// leaves the first's temporary folder alone. Once the first ends, a build of
/// the target runs: it reclaims what the first left, then fails on a refused
/// asset.
#[test]
fn a_second_build_of_one_target_is_refused_while_the_first_holds_it() -> Result<()> {
    let dir = BuildTarget::host().dir_name();
    let first_staging = format!("dist/{dir}.4242.7.tmp/half.bin");
    let project = TempProject::with(
        "one-at-a-time",
        &[
            ("Cargo.toml", b"[package]\nname = \"lantern\"\n"),
            (first_staging.as_str(), b"the first build's half"),
        ],
    )?;
    std::fs::create_dir_all(project.0.join("assets").join("materials"))?;
    std::fs::File::create(project.0.join("assets").join("materials").join("big.kmat"))?
        .set_len((64 << 20) + 1)?;
    let pvfs = crate::project_vfs::ProjectVfs::open(
        project.0.clone(),
        Arc::new(khora_sdk::MetricsRegistry::new()),
    )?;
    let signer = PackSigner::load_or_create(&project.0.join("signing"))?;
    let build = || {
        build_for_target(
            &pvfs,
            "Lantern",
            BuildTarget::host(),
            BuildPreset::Debug,
            &signer,
        )
    };
    let first = khora_sdk::fs::PathLock::try_hold(&project.0.join("dist").join(dir))?;

    let second = build();

    let line = result_line(&second);
    assert!(second.is_err(), "the second build ran beside the first");
    assert!(line.contains("another build"), "{line}");
    assert_eq!(
        std::fs::read(project.0.join(&first_staging))?,
        b"the first build's half",
        "the second build reclaimed the first's temporary folder"
    );

    drop(first);
    let third = build();

    let line = result_line(&third);
    assert!(!line.contains("another build"), "{line}");
    assert!(line.contains("big.kmat"), "{line}");
    assert!(
        !project.0.join(&first_staging).exists(),
        "once the first ended, what it left is reclaimed"
    );
    Ok(())
}
