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

//! What a path under the project's `assets/` may be: a path to an entry there,
//! and a new name given to one.

use std::path::Path;

use anyhow::{bail, Result};

use super::reached::Reached;

/// The names Windows keeps for its devices in every folder, lower case, whatever extension follows.
const DEVICE_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "conin$", "conout$", "com0", "com1", "com2", "com3", "com4",
    "com5", "com6", "com7", "com8", "com9", "com¹", "com²", "com³", "lpt0", "lpt1", "lpt2", "lpt3",
    "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9", "lpt¹", "lpt²", "lpt³",
];

/// The characters Windows refuses in a name, besides the control characters, `\` and `:`.
const RESERVED_CHARACTERS: &[char] = &['<', '>', '"', '|', '?', '*'];

/// Checks that `rel` is a forward-slash path under the folder `assets_root` whose parts that exist are each reached by the name written.
///
/// Each part between slashes is a name: not empty (no leading, doubled or
/// trailing slash, no absolute path), not `.` or `..`. Its name is never
/// held to the rules of a new one: a file made on another system keeps
/// what it is named. Each part that exists must be one the system reaches
/// under that name ([`Reached::probe`]): its folder lists it, and opening
/// it reaches that entry, not a device nor the entry the system trims the
/// name to. The parts from the first that does not exist are not looked at.
/// The source of a rename, a move or a trash, and a write over a file, are
/// checked with it.
///
/// # Errors
///
/// When `rel` is refused, naming the part refused and why.
pub fn check_existing(assets_root: &Path, rel: &str) -> Result<()> {
    check_each_part(rel, containment_fault)?;
    if let Err(predicate) = named_from(assets_root, rel.split('/'), |_| None) {
        bail!("Not a path under assets/: '{rel}' {predicate}");
    }
    Ok(())
}

/// Checks that every part of `rel` is a name a project keeps on every system, as a new name is checked.
///
/// A new name is kept as typed, never trimmed nor changed, so it must mean
/// one entry everywhere a project goes: not empty, not `.` or `..`, no `\`,
/// no `:`, no control character nor any of `<>"|?*`, no name ending with a
/// dot or a space, and no Windows device's name (`CON`, `NUL`, `COM1`, …
/// with any extension). A file or folder made, the destination of a rename,
/// a new file written and every name the editor offers are checked with it.
///
/// # Errors
///
/// When `rel` is refused, naming the part refused and why.
pub fn check_new_name(rel: &str) -> Result<()> {
    check_each_part(rel, |part| {
        containment_fault(part).or_else(|| not_portable(part))
    })
}

/// Returns `name` made a name [`check_new_name`] accepts, with any extension after it, or `fallback` when nothing of it is left.
///
/// For a name the editor makes from another (an entity's, a material's).
/// Each run of characters other than letters, digits, `_`, `-`, `.` and
/// space becomes one `_`; dots, spaces and `_` are trimmed from both ends;
/// a name Windows keeps for a device gets a `_` after its first word, so
/// `con` gives `con_` and `aux.v2` gives `aux_.v2`.
pub fn new_name_from(name: &str, fallback: &str) -> String {
    let mut out = String::with_capacity(name.len() + 1);
    let mut last_was_replacement = false;
    for ch in name.chars() {
        if ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.' | ' ') {
            out.push(ch);
            last_was_replacement = false;
        } else if !last_was_replacement {
            out.push('_');
            last_was_replacement = true;
        }
    }
    let mut kept = out.trim_matches(&[' ', '.', '_'][..]).to_owned();
    if kept.is_empty() {
        return fallback.to_owned();
    }
    let stem = kept
        .split('.')
        .next()
        .unwrap_or(&kept)
        .trim_end_matches(' ');
    if DEVICE_NAMES.contains(&stem.to_lowercase().as_str()) {
        kept.insert(stem.len(), '_');
    }
    kept
}

/// Returns `parts`, a path under the folder `root`, forward-slash: each part that exists as spelled on disk, the rest as given.
///
/// Each part that exists is one the system reaches by its name
/// ([`Reached::probe`]) and can read (no link that leads nowhere). From the
/// first that does not exist, every part is new, held to `new_part`.
///
/// # Errors
///
/// The predicate saying why a part is refused, to follow the path refused,
/// the part named last between quotes: `holds …: 'part'` or `is refused: …`.
pub(super) fn named_from<'a>(
    root: &Path,
    parts: impl IntoIterator<Item = &'a str>,
    new_part: impl Fn(&str) -> Option<&'static str>,
) -> std::result::Result<String, String> {
    let mut at = root.to_path_buf();
    let mut exists = true;
    let mut named = Vec::new();
    for part in parts {
        if exists {
            match Reached::probe(&at, part) {
                Reached::Entry(spelled) => {
                    at.push(&spelled);
                    if std::fs::metadata(&at).is_err() {
                        return Err(format!(
                            "holds an entry that cannot be read (a link that leads nowhere): \
                             '{part}'"
                        ));
                    }
                    named.push(spelled);
                    continue;
                }
                Reached::Missing => exists = false,
                refused => {
                    let why = refused.refusal(part).unwrap_or_default();
                    return Err(format!("is refused: {why}"));
                }
            }
        }
        if let Some(why) = new_part(part) {
            return Err(format!("holds {why}: '{part}'"));
        }
        named.push(part.to_owned());
    }
    Ok(named.join("/"))
}

/// Returns why `part`, a new name, is not one a project keeps on every system; `None` when it is.
pub(super) fn not_portable(part: &str) -> Option<&'static str> {
    if part.chars().any(char::is_control) {
        return Some("a control character, which no file name holds on Windows");
    }
    if part.contains(RESERVED_CHARACTERS) {
        return Some("one of <>\"|?*, which Windows refuses in a name");
    }
    if part.contains('\\') {
        return Some("a '\\', which Windows reads as another folder");
    }
    if part.contains(':') {
        return Some("a ':', which names a drive or a stream on Windows");
    }
    if part.ends_with(['.', ' ']) {
        return Some("a name ending with a dot or a space, which Windows drops");
    }
    let stem = part.split('.').next().unwrap_or(part).trim_end_matches(' ');
    if DEVICE_NAMES.contains(&stem.to_lowercase().as_str()) {
        return Some("a name Windows keeps for a device in every folder");
    }
    None
}

/// Fails on the first part of `rel` that `fault` refuses, naming it.
fn check_each_part(rel: &str, fault: impl Fn(&str) -> Option<&'static str>) -> Result<()> {
    if rel.is_empty() {
        bail!("Not a path under assets/: the path is empty");
    }
    for part in rel.split('/') {
        if let Some(why) = fault(part) {
            bail!("Not a path under assets/: '{rel}' holds {why}: '{part}'");
        }
    }
    Ok(())
}

/// Returns why the part `part` leaves its folder or names none; `None` for a name.
fn containment_fault(part: &str) -> Option<&'static str> {
    match part {
        "" => Some("an empty name (a leading, doubled or trailing '/')"),
        "." => Some("'.', which names the folder it is in"),
        ".." => Some("'..', which climbs out of its folder"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Returns an empty folder of the system's temporary folder named after `name`.
    fn temp_root(name: &str) -> std::io::Result<std::path::PathBuf> {
        let root = std::env::temp_dir().join(format!(
            "khora-editor-asset-path-{name}-{}",
            std::process::id()
        ));
        if root.exists() {
            std::fs::remove_dir_all(&root)?;
        }
        std::fs::create_dir_all(&root)?;
        Ok(root)
    }

    #[test]
    fn a_path_of_plain_names_is_accepted_as_existing_and_as_new() -> std::io::Result<()> {
        let root = temp_root("plain")?;
        for rel in [
            "a.png",
            "textures/hd/a.png",
            "scenes/level 2.kscene",
            "Tex",
            "console.erg",
            "nullable/a.png",
            "com10.png",
        ] {
            assert!(check_existing(&root, rel).is_ok(), "{rel}");
            assert!(check_new_name(rel).is_ok(), "{rel}");
        }
        Ok(())
    }

    #[test]
    fn a_path_that_leaves_its_folders_is_refused_either_way_naming_the_part() -> std::io::Result<()>
    {
        let root = temp_root("leaves")?;
        for (rel, part) in [
            ("../escaped", "'..'"),
            ("textures/../../x", "'..'"),
            ("/abs.png", "''"),
            ("textures//a.png", "''"),
            ("textures/", "''"),
            ("./a.png", "'.'"),
        ] {
            for error in [check_existing(&root, rel), check_new_name(rel)] {
                let error = error.expect_err(rel).to_string();
                assert!(error.ends_with(part), "{rel}: {error}");
            }
        }
        assert!(check_existing(&root, "").is_err());
        assert!(check_new_name("").is_err());
        Ok(())
    }

    #[test]
    fn a_new_name_a_system_reads_otherwise_is_refused_naming_the_part() {
        for (rel, part) in [
            ("textures/hd\\b.png", "'hd\\b.png'"),
            ("C:/a.png", "'C:'"),
            ("a.png:stream", "'a.png:stream'"),
            ("scenes/new.kscene.", "'new.kscene.'"),
            ("scenes /a.kscene", "'scenes '"),
            ("scripts/NUL", "'NUL'"),
            ("nul.kscene", "'nul.kscene'"),
            ("textures/aux.png", "'aux.png'"),
            ("Com1", "'Com1'"),
            ("lpt³.txt", "'lpt³.txt'"),
            ("a?.png", "'a?.png'"),
            ("a\u{7}.png", "'a\u{7}.png'"),
            ("why<not>.png", "'why<not>.png'"),
        ] {
            let error = check_new_name(rel).expect_err(rel).to_string();
            assert!(error.ends_with(part), "{rel}: {error}");
        }
    }

    #[test]
    fn an_existing_name_is_held_only_to_what_the_system_reaches_under_it() -> std::io::Result<()> {
        let root = temp_root("reached")?;
        std::fs::create_dir(root.join("v1"))?;
        std::fs::write(root.join("v1").join("a.png"), b"x")?;

        assert!(check_existing(&root, "v1/a.png").is_ok());
        // Where the name reaches `v1` (Windows trims the dot), it is refused.
        let dotted = check_existing(&root, "v1./a.png");
        assert_eq!(dotted.is_err(), root.join("v1.").exists(), "{dotted:?}");
        if let Err(error) = dotted {
            let error = error.to_string();
            assert!(error.ends_with("another entry under 'v1.'"), "{error}");
        }
        // A name no new one may take, reaching nothing, is not looked at.
        for rel in ["a?.png", "why<not>.png", "a\u{7}.png", "scripts/NUL"] {
            assert!(check_existing(&root, rel).is_ok(), "{rel}");
            assert!(check_new_name(rel).is_err(), "{rel}");
        }
        Ok(())
    }

    #[test]
    fn a_name_made_from_another_is_one_a_project_makes_with_any_extension() {
        for (name, made) in [
            ("Crate", "Crate"),
            ("my crate!", "my crate"),
            ("a/b\\c:d", "a_b_c_d"),
            ("  .hidden. ", "hidden"),
            ("con", "con_"),
            ("Aux.v2", "Aux_.v2"),
            ("nul .x", "nul_ .x"),
            ("COM1", "COM1_"),
            ("lpt³", "lpt³_"),
            ("console", "console"),
            ("???", "fallback"),
            ("", "fallback"),
        ] {
            let got = new_name_from(name, "fallback");
            assert_eq!(got, made, "{name:?}");
            for file in [got.clone(), format!("{got}.kprefab")] {
                assert!(check_new_name(&file).is_ok(), "{name:?} gives {file:?}");
            }
        }
    }

    #[test]
    fn named_from_spells_what_exists_as_on_disk_and_holds_the_rest_to_new_names(
    ) -> std::io::Result<()> {
        let root = temp_root("named-from")?;
        std::fs::create_dir(root.join("Tex"))?;

        assert_eq!(
            named_from(&root, ["Tex", "new.png"], not_portable),
            Ok("Tex/new.png".to_owned())
        );
        let other_case = named_from(&root, ["tex", "new.png"], not_portable);
        if root.join("tex").exists() {
            assert_eq!(other_case, Ok("Tex/new.png".to_owned()));
        } else {
            assert_eq!(other_case, Ok("tex/new.png".to_owned()));
        }
        let refused = named_from(&root, ["Tex", "new.png."], not_portable).expect_err("refused");
        assert!(refused.starts_with("holds "), "{refused}");
        assert!(refused.ends_with("'new.png.'"), "{refused}");
        Ok(())
    }
}
