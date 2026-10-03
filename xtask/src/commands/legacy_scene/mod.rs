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

//! `cargo xtask assets upgrade-scenes <project>`: rewrites a project's
//! scenes and prefabs saved before scene records, once.
//!
//! Each file under `<project>/assets` is read, upgraded, checked by loading
//! the result, and only then written — the original kept beside it as
//! `<file>.v1`. A file already in today's format is left as it is; one this
//! command cannot read is left as it is too, and named.

mod v1;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use khora_core::scene::{SceneFile, HEADER_MAGIC_BYTES, SCENE_FORMAT_VERSION};
use khora_io::serialization::SerializationService;
use khora_sdk::khora_data::ecs::World;
use khora_sdk::khora_data::scene::{
    capture_world, encoding_of, instantiate_subtree, serialize_subtree, write_scene_file,
    CompactEncoding, SceneRecord,
};
use walkdir::WalkDir;

use crate::term::*;

/// The strategy id a v1 recipe scene's header names.
const V1_RECIPE: &str = "KH_RECIPE_V1";

/// What became of one file.
enum Outcome {
    Upgraded { entities: usize, skipped: usize },
    Current,
}

/// Upgrades every scene and prefab under `<project>/assets`. Fails, after
/// handling every file, if any could not be upgraded.
pub fn upgrade_scenes(project: &Path, dry_run: bool) -> Result<()> {
    let assets = project.join("assets");
    if !assets.is_dir() {
        bail!("{} has no `assets` folder", project.display());
    }
    let mut files: Vec<PathBuf> = WalkDir::new(&assets)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.into_path())
        .filter(|path| path.is_file() && is_prefab(path).is_some())
        .collect();
    files.sort();

    let mut refused = Vec::new();
    for file in &files {
        let shown = file.strip_prefix(project).unwrap_or(file).display();
        match upgrade_file(file, dry_run) {
            Ok(Outcome::Upgraded { entities, skipped }) => {
                let verb = if dry_run { "would upgrade" } else { "upgraded" };
                println!(
                    "  {GREEN}{verb}{RESET} {shown} — {entities} entities{}",
                    if skipped > 0 {
                        format!(", {skipped} rebuilt component(s) not kept")
                    } else {
                        String::new()
                    }
                );
            }
            Ok(Outcome::Current) => println!("  {CYAN}current{RESET}  {shown}"),
            Err(error) => {
                println!("  {RED}refused{RESET}  {shown}: {error:#}");
                refused.push(shown.to_string());
            }
        }
    }
    if files.is_empty() {
        println!("  no scene or prefab under {}", assets.display());
    }
    if !refused.is_empty() {
        bail!(
            "{} file(s) could not be upgraded and were left as they were: {}",
            refused.len(),
            refused.join(", ")
        );
    }
    Ok(())
}

/// Reads, upgrades, checks and — unless `dry_run` — writes one file.
fn upgrade_file(file: &Path, dry_run: bool) -> Result<Outcome> {
    let bytes = fs::read(file).with_context(|| format!("reading {}", file.display()))?;
    let prefab = is_prefab(file) == Some(true);

    let recipe = if bytes.starts_with(&HEADER_MAGIC_BYTES) {
        let scene = SceneFile::from_bytes(&bytes)
            .map_err(|error| anyhow!("a damaged scene header: {error:?}"))?;
        let version = scene.header.format_version;
        if version == SCENE_FORMAT_VERSION {
            return Ok(Outcome::Current);
        }
        if version != 1 {
            bail!("scene format v{version} is not one this command reads");
        }
        let strategy = encoding_of(&scene);
        if strategy != V1_RECIPE {
            bail!(
                "a v1 `{strategy}` file: only `{V1_RECIPE}` — what the editor wrote — is upgraded"
            );
        }
        v1::read_recipe(&scene.payload)?
    } else if prefab {
        v1::read_recipe(&bytes)?
    } else {
        bail!("not a scene file");
    };

    let replayed = v1::replay(&recipe)?;
    let entities = replayed.world.iter_entities().count();
    let upgraded = if prefab {
        let root = replayed
            .first
            .ok_or_else(|| anyhow!("a prefab with no entity"))?;
        // A prefab is one tree: an entity its root does not reach would be
        // lost by the upgrade, so the file is left for an author to mend.
        let tree = khora_sdk::khora_data::scene::capture_subtree(&replayed.world, root)
            .map_err(|error| anyhow!("the prefab could not be captured: {error}"))?;
        let outside = entities - tree.entities.len();
        if outside > 0 {
            bail!("{outside} of the prefab's {entities} entities are not under its root");
        }
        let bytes = serialize_subtree(&replayed.world, root)
            .map_err(|error| anyhow!("the prefab could not be written: {error}"))?;
        check_prefab(&replayed.world, root, &bytes)?;
        bytes
    } else {
        let record = capture_world(&replayed.world)
            .map_err(|error| anyhow!("the scene could not be captured: {error}"))?;
        let scene = write_scene_file(&record, &CompactEncoding)
            .map_err(|error| anyhow!("the scene could not be written: {error}"))?;
        check_scene(&record, &scene)?;
        scene.to_bytes()
    };

    // The original is kept as `<file>.v1`; one already there must be this
    // very file, or keeping it would cost the original.
    let backup = beside(file, "v1");
    if let Ok(kept) = fs::read(&backup) {
        if kept != bytes {
            bail!(
                "{} already exists and holds another file: move it away and run again",
                backup.display()
            );
        }
    }
    if !dry_run {
        write_beside(file, &backup, &bytes, &upgraded)?;
    }
    Ok(Outcome::Upgraded {
        entities,
        skipped: replayed.skipped,
    })
}

/// Loads the upgraded scene back and compares what it holds with what was
/// written: each component on as many entities.
fn check_scene(written: &SceneRecord, scene: &SceneFile) -> Result<()> {
    let mut world = World::new();
    SerializationService::new()
        .load_world(scene, &mut world)
        .map_err(|error| anyhow!("the upgraded scene does not load: {error}"))?;
    let reloaded = capture_world(&world)
        .map_err(|error| anyhow!("the upgraded scene does not capture: {error}"))?;
    if census(&reloaded) != census(written) {
        bail!("the upgraded scene does not load back the same");
    }
    Ok(())
}

/// Instantiates the upgraded prefab and compares it with the source tree.
fn check_prefab(
    source: &World,
    root: khora_core::ecs::entity::EntityId,
    bytes: &[u8],
) -> Result<()> {
    let mut world = World::new();
    instantiate_subtree(&mut world, bytes)
        .map_err(|error| anyhow!("the upgraded prefab does not instantiate: {error}"))?;
    let written = khora_sdk::khora_data::scene::capture_subtree(source, root)
        .map_err(|error| anyhow!("the prefab does not capture: {error}"))?;
    let reloaded = capture_world(&world)
        .map_err(|error| anyhow!("the upgraded prefab does not capture: {error}"))?;
    if census(&reloaded) != census(&written) {
        bail!("the upgraded prefab does not instantiate the same");
    }
    Ok(())
}

/// Each component name and how many entities hold it, sorted.
fn census(record: &SceneRecord) -> Vec<(String, usize)> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for page in &record.pages {
        for name in &page.components {
            match counts.iter_mut().find(|(known, _)| known == name) {
                Some((_, count)) => *count += page.rows.len(),
                None => counts.push((name.clone(), page.rows.len())),
            }
        }
    }
    counts.push(("<entities>".to_owned(), record.entities.len()));
    counts.sort();
    counts
}

/// Whether `path` is a prefab (`Some(true)`), a scene (`Some(false)`), or
/// neither — by its extension, in any case, as the engine reads it.
fn is_prefab(path: &Path) -> Option<bool> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "kprefab" => Some(true),
        "kscene" => Some(false),
        _ => None,
    }
}

/// `<file>.<suffix>`.
fn beside(file: &Path, suffix: &str) -> PathBuf {
    let mut path = file.as_os_str().to_owned();
    path.push(".");
    path.push(suffix);
    PathBuf::from(path)
}

/// Keeps `original` as `backup`, then puts `upgraded` in place of `file`
/// through a temporary file, so the file is never half-written. If the
/// file cannot be replaced, nothing this wrote stays beside it.
fn write_beside(file: &Path, backup: &Path, original: &[u8], upgraded: &[u8]) -> Result<()> {
    let created_backup = !backup.exists();
    if created_backup {
        fs::write(backup, original).with_context(|| format!("writing {}", backup.display()))?;
    }
    let temporary = beside(file, "upgrading");
    let replaced = fs::write(&temporary, upgraded)
        .with_context(|| format!("writing {}", temporary.display()))
        .and_then(|()| {
            fs::rename(&temporary, file).with_context(|| format!("replacing {}", file.display()))
        });
    if replaced.is_err() {
        let _ = fs::remove_file(&temporary);
        if created_backup {
            let _ = fs::remove_file(backup);
        }
    }
    replaced
}
