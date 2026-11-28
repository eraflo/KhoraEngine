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

//! What a file operation of the asset browser did to the keys under `assets/`.

use crate::asset::AssetUUID;

/// What a file operation did to the forward-slash keys under `assets/`: an entry moved, or trashed.
///
/// The entry is a file or a folder; what a folder holds goes with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyChange {
    /// The entry at `from` is at `to` now.
    Moved {
        /// The entry's key before the move.
        from: String,
        /// The entry's key after the move.
        to: String,
    },
    /// The entry at `key` went to the recycle bin.
    Trashed {
        /// The entry's key.
        key: String,
        /// The identities of the files it held: itself, for a file.
        identities: Vec<AssetUUID>,
    },
}

impl KeyChange {
    /// Returns the key `key` names after the change: the same, moved, or `None` when trashed.
    pub fn follow(&self, key: &str) -> Option<String> {
        match self {
            Self::Moved { from, to } => Some(match under(key, from) {
                Some(rest) => format!("{to}{rest}"),
                None => key.to_owned(),
            }),
            Self::Trashed { key: gone, .. } => under(key, gone).is_none().then(|| key.to_owned()),
        }
    }

    /// Returns the folder to show in place of `folder` after the change: moved along, or its nearest parent left when trashed.
    ///
    /// `""` is the root of `assets/`, which no change trashes.
    pub fn follow_folder(&self, folder: &str) -> String {
        let mut at = folder;
        loop {
            if let Some(followed) = self.follow(at) {
                return followed;
            }
            at = at.rfind('/').map_or("", |slash| &at[..slash]);
        }
    }

    /// Returns whether the change sent the asset `uuid` to the recycle bin.
    pub fn trashed(&self, uuid: AssetUUID) -> bool {
        match self {
            Self::Moved { .. } => false,
            Self::Trashed { identities, .. } => identities.contains(&uuid),
        }
    }
}

/// Returns what follows `entry` in `key` when `key` is `entry` or lies under it: `""`, or `/` and the rest.
fn under<'a>(key: &'a str, entry: &str) -> Option<&'a str> {
    if entry.is_empty() {
        return None;
    }
    let rest = key.strip_prefix(entry)?;
    (rest.is_empty() || rest.starts_with('/')).then_some(rest)
}

#[cfg(test)]
mod tests {
    use crate::asset::AssetUUID;

    use super::KeyChange;

    fn moved(from: &str, to: &str) -> KeyChange {
        KeyChange::Moved {
            from: from.to_owned(),
            to: to.to_owned(),
        }
    }

    fn trashed(key: &str) -> KeyChange {
        KeyChange::Trashed {
            key: key.to_owned(),
            identities: Vec::new(),
        }
    }

    /// A file moved is followed to its new key; any other key stays.
    #[test]
    fn follow_names_a_moved_file_by_its_new_key() {
        let change = moved("scenes/level.kscene", "levels/level.kscene");

        assert_eq!(
            change.follow("scenes/level.kscene").as_deref(),
            Some("levels/level.kscene")
        );
        assert_eq!(
            change.follow("scenes/other.kscene").as_deref(),
            Some("scenes/other.kscene")
        );
    }

    /// A folder moved takes every key under it along, and no key that only starts with its name.
    #[test]
    fn follow_takes_what_a_moved_folder_holds_along() {
        let change = moved("scenes", "levels");

        assert_eq!(
            change.follow("scenes/a/b.kscene").as_deref(),
            Some("levels/a/b.kscene")
        );
        assert_eq!(change.follow("scenes").as_deref(), Some("levels"));
        assert_eq!(
            change.follow("scenes2/b.kscene").as_deref(),
            Some("scenes2/b.kscene")
        );
    }

    /// A key trashed, or under a folder trashed, is gone; the others stay.
    #[test]
    fn follow_forgets_what_was_trashed() {
        let change = trashed("scenes");

        assert_eq!(change.follow("scenes/level.kscene"), None);
        assert_eq!(change.follow("scenes"), None);
        assert_eq!(
            change.follow("scenes.kscene").as_deref(),
            Some("scenes.kscene")
        );
    }

    /// A folder moved, or under a folder moved, is followed where it went; another stays.
    #[test]
    fn follow_folder_takes_a_folder_along_its_move() {
        let change = moved("levels", "worlds");

        assert_eq!(change.follow_folder("levels/forest"), "worlds/forest");
        assert_eq!(change.follow_folder("levels"), "worlds");
        assert_eq!(change.follow_folder("props"), "props");
        assert_eq!(change.follow_folder(""), "");
    }

    /// A folder trashed, or under a folder trashed, gives way to its nearest parent left.
    #[test]
    fn follow_folder_falls_back_to_the_nearest_parent_left_by_a_trash() {
        assert_eq!(
            trashed("levels/forest").follow_folder("levels/forest/caves"),
            "levels"
        );
        assert_eq!(
            trashed("levels/forest").follow_folder("levels/forest"),
            "levels"
        );
        assert_eq!(trashed("levels").follow_folder("levels/forest"), "");
        assert_eq!(trashed("levels").follow_folder("props"), "props");
    }

    /// The identities a trash names are the ones it trashed; a move trashes none.
    #[test]
    fn trashed_names_the_identities_sent_to_the_bin() {
        let gone = AssetUUID::new_v5("materials/a.kmat");
        let kept = AssetUUID::new_v5("materials/b.kmat");
        let change = KeyChange::Trashed {
            key: "materials/a.kmat".to_owned(),
            identities: vec![gone],
        };

        assert!(change.trashed(gone));
        assert!(!change.trashed(kept));
        assert!(!moved("materials/a.kmat", "materials/c.kmat").trashed(gone));
    }
}
