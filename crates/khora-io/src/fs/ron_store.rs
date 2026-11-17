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

//! A list of entries kept in one RON file that several programs share.

use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

use super::{make_parent, read_bounded_to_string, write_atomic, FileStamp};

/// The largest store read: a store is a few entries.
const MAX_STORE_BYTES: u64 = 4 << 20;

/// An entry of a [`RonStore`]: what it is held by.
pub trait Keyed {
    /// What tells two entries apart: a store holds one entry per key.
    type Key: Ord + Clone;

    /// Returns what this entry is held by.
    fn key(&self) -> Self::Key;
}

/// A list of entries in one RON file, which several programs may share.
///
/// Each write reads the file again and applies only its own changes, so it
/// keeps what the others wrote; the file is replaced whole. A file that does
/// not read, or is over 4 MiB, reads as empty, with a log line.
pub struct RonStore<E> {
    path: PathBuf,
    /// What the store is, for the log: `trust store`, `quarantine list`.
    what: &'static str,
    entries: PhantomData<fn() -> E>,
}

impl<E: Keyed + Serialize + DeserializeOwned> RonStore<E> {
    /// Returns the store kept in the file at `path`, called `what` in the log.
    pub fn new(path: PathBuf, what: &'static str) -> Self {
        Self {
            path,
            what,
            entries: PhantomData,
        }
    }

    /// Returns the file the store is kept in.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the store file's stamp; `None` when there is no file.
    pub fn stamp(&self) -> Option<FileStamp> {
        FileStamp::of(&self.path)
    }

    /// Returns the entries the file holds now, by key: none when there is no file.
    ///
    /// A file that cannot be read, is too large or is not a list of entries is
    /// logged and read as empty.
    pub fn read(&self) -> BTreeMap<E::Key, E> {
        let text = match read_bounded_to_string(&self.path, MAX_STORE_BYTES) {
            Ok(text) => text,
            Err(error) if error.is_not_found() => return BTreeMap::new(),
            Err(error) => {
                log::warn!("{} is not read: {error}", self.path.display());
                return BTreeMap::new();
            }
        };
        match ron::from_str::<Vec<E>>(&text) {
            Ok(entries) => entries
                .into_iter()
                .map(|entry| (entry.key(), entry))
                .collect(),
            Err(error) => {
                log::warn!("{} is not a {}: {error}", self.path.display(), self.what);
                BTreeMap::new()
            }
        }
    }

    /// Applies `changes` over the file as it is now, and writes it whole.
    ///
    /// A change puts its entry at its key, or with `None` takes the key out.
    /// Returns the entries merged, written or not, with how the write went.
    /// Writers merging at once, in this process or another, take turns.
    pub fn merge(
        &self,
        changes: impl IntoIterator<Item = (E::Key, Option<E>)>,
    ) -> (BTreeMap<E::Key, E>, std::io::Result<()>) {
        // Another writer, in this process or another, merges after this one.
        let _held = self.hold();
        let mut merged = self.read();
        for (key, entry) in changes {
            match entry {
                Some(entry) => merged.insert(key, entry),
                None => merged.remove(&key),
            };
        }
        let written = self.write(&merged);
        (merged, written)
    }

    /// Locks the file beside the store, `<store>.lock`, until the returned file is dropped.
    ///
    /// `None`, logged, when it cannot be locked: the merge then goes on unlocked.
    fn hold(&self) -> Option<std::fs::File> {
        let mut name = self.path.file_name()?.to_os_string();
        name.push(".lock");
        let lock_path = self.path.with_file_name(name);
        let held = make_parent(&lock_path)
            .and_then(|()| {
                std::fs::OpenOptions::new()
                    .create(true)
                    .truncate(false)
                    .write(true)
                    .open(&lock_path)
            })
            .and_then(|file| file.lock().map(|()| file));
        match held {
            Ok(file) => Some(file),
            Err(error) => {
                log::warn!("{} is not locked: {error}", lock_path.display());
                None
            }
        }
    }

    /// Writes `entries` to the file whole, sorted by key.
    fn write(&self, entries: &BTreeMap<E::Key, E>) -> std::io::Result<()> {
        let list: Vec<&E> = entries.values().collect();
        let text = ron::ser::to_string_pretty(&list, ron::ser::PrettyConfig::default())
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        write_atomic(&self.path, text.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Note {
        name: String,
        value: u32,
    }

    impl Keyed for Note {
        type Key = String;
        fn key(&self) -> String {
            self.name.clone()
        }
    }

    fn note(name: &str, value: u32) -> Note {
        Note {
            name: name.to_owned(),
            value,
        }
    }

    fn store(dir: &Path) -> RonStore<Note> {
        RonStore::new(dir.join("nested").join("notes.ron"), "note list")
    }

    #[test]
    fn a_store_with_no_file_reads_empty_and_has_no_stamp() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        assert!(store.read().is_empty());
        assert_eq!(store.stamp(), None);
    }

    #[test]
    fn a_merge_writes_its_changes_and_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        let (merged, written) = store.merge([
            ("b".to_owned(), Some(note("b", 2))),
            ("a".to_owned(), Some(note("a", 1))),
        ]);
        written.expect("the store is written, its folder made");
        assert_eq!(merged.len(), 2);
        assert_eq!(store.read(), merged);
        assert!(store.stamp().is_some());
    }

    #[test]
    fn a_merge_keeps_what_another_writer_wrote_since() {
        let dir = tempfile::tempdir().unwrap();
        let ours = store(dir.path());
        let theirs = store(dir.path());
        ours.merge([("a".to_owned(), Some(note("a", 1)))])
            .1
            .unwrap();
        theirs
            .merge([("b".to_owned(), Some(note("b", 2)))])
            .1
            .unwrap();
        let (merged, written) = ours.merge([("a".to_owned(), None)]);
        written.unwrap();
        assert_eq!(
            merged.into_values().collect::<Vec<_>>(),
            vec![note("b", 2)],
            "a removed by us, b kept from them"
        );
    }

    /// Writers merging at once through their own handles all keep their entry.
    ///
    /// None reads the file before another's write and writes over it.
    #[test]
    fn merges_at_once_from_several_handles_each_keep_their_entry() {
        for _ in 0..10 {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("notes.ron");
            let ready = std::sync::Barrier::new(8);
            let landed: Vec<String> = std::thread::scope(|scope| {
                let writers: Vec<_> = (0..8u32)
                    .map(|i| {
                        let (path, ready) = (&path, &ready);
                        scope.spawn(move || {
                            let store = RonStore::<Note>::new(path.clone(), "note list");
                            let name = format!("n{i}");
                            ready.wait();
                            let (_, written) = store.merge([(name.clone(), Some(note(&name, i)))]);
                            written.ok().map(|()| name)
                        })
                    })
                    .collect();
                writers
                    .into_iter()
                    .filter_map(|writer| writer.join().unwrap())
                    .collect()
            });
            let read = RonStore::<Note>::new(path, "note list").read();
            for name in &landed {
                assert!(
                    read.contains_key(name),
                    "{name} was written, yet the store holds only {:?}",
                    read.keys().collect::<Vec<_>>()
                );
            }
        }
    }

    #[test]
    fn a_store_that_does_not_parse_reads_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        std::fs::write(store.path(), "not ron [").unwrap();
        assert!(store.read().is_empty());
    }

    #[test]
    fn a_store_over_its_size_reads_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        let file = std::fs::File::create(store.path()).unwrap();
        file.set_len(MAX_STORE_BYTES + 1).unwrap();
        assert!(store.read().is_empty());
    }

    /// The store and the lock file writers take turns on are all a merge leaves.
    #[test]
    fn a_merge_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        store
            .merge([("a".to_owned(), Some(note("a", 1)))])
            .1
            .unwrap();
        let mut names: Vec<_> = std::fs::read_dir(store.path().parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                std::ffi::OsString::from("notes.ron"),
                std::ffi::OsString::from("notes.ron.lock")
            ]
        );
    }
}
