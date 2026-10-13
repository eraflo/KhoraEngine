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

//! `cargo xtask assets upgrade-behaviors <project>`: a one-time upgrade of a
//! project's scenes, prefabs and game saves from the legacy `Script` /
//! `ScriptState` components to behavior components — `Guard` and its state
//! companion `Guard#state`.
//!
//! Each file is loaded into a world that holds what the project's scripts
//! declare, converted there, and written back by the engine's own writers:
//! prefabs first, so a scene is written against the prefabs it links as they
//! now are, then scenes, then saves against their converted scene. Nothing is
//! written unless every file converts.

mod convert;
mod library;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use khora_core::asset::{asset_key, AssetUUID};
use khora_core::scene::{SceneFile, SerializationGoal, HEADER_MAGIC_BYTES, SAVE_MAGIC_BYTES};
use khora_io::asset::AssetIdRegistry;
use khora_io::script::declarations::{
    apply_declarations, declaration_channel, DeclaredComponents, ScriptDeclarations,
};
use khora_io::script::hot_reload::{load_all, reload_channel};
use khora_io::serialization::SerializationService;
use khora_sdk::khora_data::ecs::World;
use khora_sdk::khora_data::scene::record::LoadReport;
use khora_sdk::khora_data::scene::snapshot::SNAPSHOT_ENCODING_ID;
use khora_sdk::khora_data::scene::{
    encoding_of, read_save_file, read_scene_file, serialize_prefab, SceneRecord,
};

use convert::{convert, detach, Converted};
use library::Library;

/// A scene or prefab file of the project.
struct SceneEntry {
    path: PathBuf,
    id: AssetUUID,
    file: SceneFile,
    record: SceneRecord,
}

/// What the upgrade will write, and what it found on the way.
#[derive(Default)]
struct Plan {
    writes: Vec<(PathBuf, Vec<u8>)>,
    lines: Vec<String>,
    warnings: Vec<String>,
    errors: Vec<String>,
}

/// Converts every scene, prefab and game save of `project` — the saves found
/// under each of `saves` — or, with `dry_run`, only prints what it would
/// convert. All-or-nothing: a file that cannot be converted leaves every file
/// as it was.
pub fn run(project: &Path, saves: &[PathBuf], dry_run: bool) -> Result<()> {
    let assets = project.join("assets");
    if !assets.is_dir() {
        bail!("{} has no `assets/` directory", project.display());
    }
    let (batch, uncompiled) = declarations(&assets.join("scripts"));

    let registry = AssetIdRegistry::load(project);
    let mut plan = Plan::default();
    let mut scenes = Vec::new();
    let mut save_files = Vec::new();
    let roots = std::iter::once(assets.clone()).chain(saves.iter().cloned());
    // Each file once, however many of the roots reach it.
    let mut seen = HashSet::new();
    let found = roots
        .flat_map(|root| files_under(&root))
        .filter(|path| seen.insert(fs::canonicalize(path).unwrap_or_else(|_| path.clone())));
    for path in found {
        let Ok(bytes) = fs::read(&path) else { continue };
        let Ok(file) = SceneFile::from_bytes(&bytes) else {
            continue;
        };
        let shown = shown(project, &path);
        if file.header.magic_bytes == SAVE_MAGIC_BYTES {
            save_files.push((path, file));
        } else if file.header.magic_bytes == HEADER_MAGIC_BYTES {
            if encoding_of(&file) == SNAPSHOT_ENCODING_ID {
                plan.warnings.push(format!(
                    "{shown}: a snapshot, a same-build cache — skipped; write it again from its scene"
                ));
                continue;
            }
            let Ok(under) = path.strip_prefix(&assets) else {
                continue;
            };
            match read_scene_file(&file) {
                Ok(record) => scenes.push(SceneEntry {
                    id: registry.resolve(&asset_key(under)),
                    path,
                    file,
                    record,
                }),
                Err(error) => plan.errors.push(format!("{shown}: unreadable: {error}")),
            }
        }
    }

    let old = Library(
        scenes
            .iter()
            .map(|scene| (scene.id, scene.record.clone()))
            .collect(),
    );
    let mut new = old.clone();
    let mut new_files: HashMap<AssetUUID, SceneFile> = HashMap::new();
    for at in order(&scenes, project, &mut plan) {
        let scene = &scenes[at];
        if let Some(file) = upgrade_scene(
            scene, project, &batch, &old, &mut new, &new_files, &mut plan,
        ) {
            new_files.insert(scene.id, file);
        }
    }
    for (path, file) in &save_files {
        upgrade_save(
            path, file, project, &batch, &scenes, &old, &new, &new_files, &mut plan,
        );
    }

    for line in &plan.lines {
        println!("{line}");
    }
    for warning in &plan.warnings {
        println!("warning: {warning}");
    }
    if !plan.errors.is_empty() {
        for error in &plan.errors {
            eprintln!("error: {error}");
        }
        if !uncompiled.is_empty() {
            eprintln!("these scripts did not compile: {}", uncompiled.join(", "));
        }
        bail!(
            "{} file(s) could not be converted; nothing was written",
            plan.errors.len()
        );
    }
    let total = plan.writes.len();
    if dry_run {
        println!("dry run: {total} file(s) would be converted, nothing written");
        return Ok(());
    }
    write_all(&plan.writes)?;
    println!("{total} file(s) converted");
    Ok(())
}

/// What the project's scripts declare, and the scripts that did not compile.
fn declarations(scripts: &Path) -> (Vec<ScriptDeclarations>, Vec<String>) {
    let reloads = reload_channel();
    let declarations = declaration_channel();
    load_all(scripts, &reloads, &declarations);
    let compiled: HashSet<String> = reloads
        .drain()
        .into_iter()
        .map(|reload| reload.module)
        .collect();
    let uncompiled = files_under(scripts)
        .into_iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "erg"))
        .filter_map(|path| {
            let module = path
                .strip_prefix(scripts)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/");
            (!compiled.contains(&module)).then_some(module)
        })
        .collect();
    (declarations.drain(), uncompiled)
}

/// A world holding what the project's scripts declare.
fn declared_world(batch: &[ScriptDeclarations]) -> World {
    let mut world = World::new();
    apply_declarations(&mut world, &mut DeclaredComponents::default(), batch);
    world
}

/// The scene files in the order they are converted: each after the prefabs
/// it links. A file in a cycle of links is an error.
fn order(scenes: &[SceneEntry], project: &Path, plan: &mut Plan) -> Vec<usize> {
    let at: HashMap<AssetUUID, usize> = scenes.iter().enumerate().map(|(i, s)| (s.id, i)).collect();
    let mut done = vec![false; scenes.len()];
    let mut order = Vec::new();
    fn visit(
        i: usize,
        scenes: &[SceneEntry],
        at: &HashMap<AssetUUID, usize>,
        done: &mut [bool],
        on_path: &mut Vec<usize>,
        order: &mut Vec<usize>,
    ) -> bool {
        if done[i] {
            return true;
        }
        if on_path.contains(&i) {
            return false;
        }
        on_path.push(i);
        for instance in &scenes[i].record.instances {
            if let Some(&dep) = at.get(&instance.prefab) {
                if !visit(dep, scenes, at, done, on_path, order) {
                    return false;
                }
            }
        }
        on_path.pop();
        done[i] = true;
        order.push(i);
        true
    }
    for i in 0..scenes.len() {
        let mut on_path = Vec::new();
        if !visit(i, scenes, &at, &mut done, &mut on_path, &mut order) {
            plan.errors.push(format!(
                "{}: its prefabs link each other in a cycle",
                shown(project, &scenes[i].path)
            ));
        }
    }
    order
}

/// Converts one scene or prefab file; `None` when it holds nothing to
/// convert, and is left as it is.
fn upgrade_scene(
    scene: &SceneEntry,
    project: &Path,
    batch: &[ScriptDeclarations],
    old: &Library,
    new: &mut Library,
    rewritten: &HashMap<AssetUUID, SceneFile>,
    plan: &mut Plan,
) -> Option<SceneFile> {
    let shown = shown(project, &scene.path);
    let mut world = declared_world(batch);
    let report = match SerializationService::new().load_world_with(&scene.file, &mut world, old) {
        Ok(report) => report,
        Err(error) => {
            plan.errors
                .push(format!("{shown}: cannot be loaded: {error:?}"));
            return None;
        }
    };
    let converted = convert(&mut world);
    // A file stored as differences from a prefab that was rewritten is
    // written again against it, whatever it holds: an instance that removed
    // the prefab's `Script` now removes its behavior.
    let against_rewritten = scene
        .record
        .instances
        .iter()
        .any(|instance| rewritten.contains_key(&instance.prefab));
    if !take(&shown, &converted, &report, against_rewritten, plan) {
        return None;
    }
    let encoding = encoding_of(&scene.file);
    let is_prefab = scene.path.extension().is_some_and(|ext| ext == "kprefab");
    let written = if is_prefab {
        let root = scene
            .record
            .entities
            .first()
            .and_then(|id| world.entity_with_id(*id));
        let Some(root) = root else {
            plan.errors
                .push(format!("{shown}: the prefab's root is missing"));
            return None;
        };
        serialize_prefab(&world, root, scene.id, new)
            .map_err(|error| format!("{error:?}"))
            .and_then(|bytes| SceneFile::from_bytes(&bytes).map_err(|error| format!("{error:?}")))
    } else {
        SerializationService::new()
            .save_world_with(&world, goal_of(&encoding), new)
            .map_err(|error| format!("{error:?}"))
    };
    let file = match written {
        Ok(file) => file,
        Err(error) => {
            plan.errors
                .push(format!("{shown}: cannot be written: {error}"));
            return None;
        }
    };
    match read_scene_file(&file) {
        Ok(record) => {
            new.0.insert(scene.id, record);
        }
        Err(error) => {
            plan.errors
                .push(format!("{shown}: does not read back: {error}"));
            return None;
        }
    }
    let now = encoding_of(&file);
    let encoding_note = if now == encoding {
        now
    } else {
        format!("{encoding} → {now}")
    };
    plan.lines.push(format!(
        "{shown} ({}, {encoding_note}): {}",
        if is_prefab { "prefab" } else { "scene" },
        summary(&converted)
    ));
    plan.writes.push((scene.path.clone(), file.to_bytes()));
    Some(file)
}

/// Converts one game save, against its scene as converted.
#[allow(clippy::too_many_arguments)]
fn upgrade_save(
    path: &Path,
    file: &SceneFile,
    project: &Path,
    batch: &[ScriptDeclarations],
    scenes: &[SceneEntry],
    old: &Library,
    new: &Library,
    new_files: &HashMap<AssetUUID, SceneFile>,
    plan: &mut Plan,
) {
    let shown = shown(project, path);
    let base_id = match SerializationService::new().save_base(file) {
        Ok(id) => id,
        Err(error) => {
            plan.errors
                .push(format!("{shown}: unreadable save: {error:?}"));
            return;
        }
    };
    let Some(base) = scenes.iter().find(|scene| scene.id == base_id) else {
        plan.errors.push(format!(
            "{shown}: its scene ({base_id:?}) is not in the project"
        ));
        return;
    };
    let mut world = declared_world(batch);
    let report = match SerializationService::with_prefabs(Arc::new(old.clone()))
        .load_game(&mut world, file, &base.file)
    {
        Ok(report) => report,
        Err(error) => {
            plan.errors
                .push(format!("{shown}: cannot be loaded: {error:?}"));
            return;
        }
    };
    let mut converted = convert(&mut world);
    // The game removed a `Script` its scene had: what the scene holds of it
    // now — converted in an earlier run — goes too.
    if let Ok(save) = read_save_file(file) {
        let removed = save
            .removed
            .iter()
            .filter(|entry| entry.components.iter().any(|name| name == "Script"))
            .filter_map(|entry| world.entity_with_id(entry.entity))
            .collect::<Vec<_>>();
        detach(&mut world, removed, &mut converted);
    }
    // A save is the differences from its scene: written again against the
    // scene as rewritten, whatever it holds.
    if !take(
        &shown,
        &converted,
        &report,
        new_files.contains_key(&base_id),
        plan,
    ) {
        return;
    }
    let base_file = new_files.get(&base_id).unwrap_or(&base.file);
    let encoding = encoding_of(file);
    let written = SerializationService::with_prefabs(Arc::new(new.clone())).save_game(
        &world,
        base_id,
        base_file,
        goal_of(&encoding),
    );
    match written {
        Ok(saved) => {
            plan.lines.push(format!(
                "{shown} (save, {encoding}): {}",
                summary(&converted)
            ));
            plan.writes.push((path.to_path_buf(), saved.to_bytes()));
        }
        Err(error) => plan
            .errors
            .push(format!("{shown}: cannot be written: {error:?}")),
    }
}

/// Takes one file's conversion into the plan: whether it is to be written —
/// it converted something, or it is `against_rewritten`, differences from a
/// file written again.
fn take(
    shown: &str,
    converted: &Converted,
    report: &LoadReport,
    against_rewritten: bool,
    plan: &mut Plan,
) -> bool {
    plan.errors.extend(
        converted
            .errors
            .iter()
            .map(|error| format!("{shown}: {error}")),
    );
    plan.warnings.extend(
        converted
            .warnings
            .iter()
            .map(|warning| format!("{shown}: {warning}")),
    );
    if !converted.errors.is_empty() || (converted.is_empty() && !against_rewritten) {
        return false;
    }
    plan.warnings.extend(
        report
            .entries
            .iter()
            .map(|entry| format!("{shown}: on loading, {entry}")),
    );
    true
}

/// What one file's conversion did, for its report line.
fn summary(converted: &Converted) -> String {
    if converted.is_empty() {
        return "written again against what it links, converted".to_owned();
    }
    let mut parts = vec![format!("{} Script → behavior", converted.scripts)];
    if converted.states > 0 {
        parts.push(format!("{} ScriptState → #state", converted.states));
    }
    if converted.dropped > 0 {
        parts.push(format!("{} ScriptState dropped", converted.dropped));
    }
    if converted.detached > 0 {
        parts.push(format!(
            "{} behavior removed with its Script",
            converted.detached
        ));
    }
    parts.join(", ")
}

/// The goal that writes a file in `encoding` again.
fn goal_of(encoding: &str) -> SerializationGoal {
    match encoding {
        "KH_TEXT_V2" => SerializationGoal::HumanReadableDebug,
        "KH_MSGPACK_V2" => SerializationGoal::PortableBinary,
        _ => SerializationGoal::EditorInterchange,
    }
}

/// Every file under `root`, skipping the editor's and version control's.
fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                let name = entry.file_name();
                if name != ".khora" && name != ".git" && name != "target" {
                    dirs.push(path);
                }
            } else {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// `path` as the report shows it: relative to the project.
fn shown(project: &Path, path: &Path) -> String {
    path.strip_prefix(project)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Where `path` is written before it replaces the file: beside it, under
/// its whole name.
fn staging(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".upgrade-tmp");
    path.with_file_name(name)
}

/// Writes every file of `writes`, or — as far as the file system lets it —
/// none: each is written to a temporary file beside it and every target is
/// checked replaceable before the first is replaced; on a failure the
/// temporary files are removed.
fn write_all(writes: &[(PathBuf, Vec<u8>)]) -> Result<()> {
    let staged: Vec<(PathBuf, &PathBuf)> = writes
        .iter()
        .map(|(path, _)| (staging(path), path))
        .collect();
    if let Some((tmp, _)) = staged.iter().find(|(tmp, _)| tmp.exists()) {
        bail!(
            "{} exists, and is where a file is written before it replaces another; nothing was written",
            tmp.display()
        );
    }
    let clean = || {
        for (tmp, _) in &staged {
            let _ = fs::remove_file(tmp);
        }
    };
    for ((tmp, path), (_, bytes)) in staged.iter().zip(writes) {
        let replaceable = fs::metadata(path).map_or(true, |meta| !meta.permissions().readonly());
        let written = if replaceable {
            fs::write(tmp, bytes).with_context(|| format!("writing {}", tmp.display()))
        } else {
            Err(anyhow::anyhow!("{} is read-only", path.display()))
        };
        if let Err(error) = written {
            clean();
            return Err(error.context("nothing was written"));
        }
    }
    for (done, (tmp, path)) in staged.iter().enumerate() {
        if let Err(error) = fs::rename(tmp, path) {
            clean();
            let replaced: Vec<String> = staged[..done]
                .iter()
                .map(|(_, path)| path.display().to_string())
                .collect();
            bail!(
                "replacing {}: {error}; already replaced: {}",
                path.display(),
                if replaced.is_empty() {
                    "none".to_owned()
                } else {
                    replaced.join(", ")
                }
            );
        }
    }
    Ok(())
}
