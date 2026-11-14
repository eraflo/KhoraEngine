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

//! The file guard: every file the engine reads from a project is read
//! through one.

use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use super::{
    size_cap, FileKind, QuarantineKey, Refusals, SecurityReason, SecurityRefusal, Trust, TrustLayer,
};

/// Reads the files under one root, refusing what leaves it, what is over its
/// cap and what is quarantined.
///
/// A file carrying the system's download mark is read as [`Trust::Imported`]
/// when that is stricter than the guard's trust. Every refusal but
/// `Quarantined` goes to the [`Refusals`]: a quarantined file was reported
/// when it was quarantined.
///
/// See the book: concepts/trust.md, "The guard".
pub struct FileGuard {
    /// The root as it was given: what refusals and the quarantine name files
    /// by.
    root: PathBuf,
    trust: Trust,
    refusals: Arc<Refusals>,
}

/// Why a [`FileGuard`] read returned no bytes.
#[derive(Debug)]
pub enum ReadError {
    /// The guard refused the file.
    Refused(SecurityRefusal),
    /// The file could not be read: missing, or the system refused it.
    Io(std::io::Error),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::Refused(refusal) => write!(f, "refused: {refusal}"),
            ReadError::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ReadError {}

impl FileGuard {
    /// A guard reading under `root` at `trust`, reporting its refusals to
    /// `refusals`.
    pub fn new(root: impl Into<PathBuf>, trust: Trust, refusals: Arc<Refusals>) -> Self {
        Self {
            root: root.into(),
            trust,
            refusals,
        }
    }

    /// The root the guard reads under.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The trust the guard reads at.
    pub fn trust(&self) -> Trust {
        self.trust
    }

    /// The refusals the guard reports to.
    pub fn refusals(&self) -> &Arc<Refusals> {
        &self.refusals
    }

    /// Returns the bytes of the file `rel` names under the root.
    ///
    /// # Errors
    ///
    /// [`ReadError::Refused`] when the guard refuses the file,
    /// [`ReadError::Io`] when it cannot be read.
    pub fn read(&self, rel: impl AsRef<Path>) -> Result<Vec<u8>, ReadError> {
        let rel = rel.as_ref();
        let named = self.root.join(rel);
        if leaves(rel) {
            return Err(self.refuse(named, self.trust, SecurityReason::Escape, None));
        }
        let real_root = self.root.canonicalize().map_err(ReadError::Io)?;
        let real = named.canonicalize().map_err(ReadError::Io)?;
        if !real.starts_with(&real_root) {
            return Err(self.refuse(named, self.trust, SecurityReason::Escape, None));
        }

        let trust = if downloaded(&real) && !self.trust.is_strict() {
            Trust::Imported
        } else {
            self.trust
        };
        let cap = size_cap(FileKind::of_path(rel), trust);
        let file = std::fs::File::open(&real).map_err(ReadError::Io)?;
        let metadata = file.metadata().map_err(ReadError::Io)?;
        if metadata.len() > cap {
            let stamp = metadata
                .modified()
                .ok()
                .map(|modified| QuarantineKey::Stamp {
                    len: metadata.len(),
                    modified,
                });
            let reason = SecurityReason::TooLarge {
                size: metadata.len(),
                cap,
            };
            return Err(self.refuse(named, trust, reason, stamp));
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.take(cap + 1)
            .read_to_end(&mut bytes)
            .map_err(ReadError::Io)?;
        if bytes.len() as u64 > cap {
            // It grew past its cap since its size was read.
            let reason = SecurityReason::TooLarge {
                size: bytes.len() as u64,
                cap,
            };
            return Err(self.refuse(named, trust, reason, None));
        }

        if self.refusals.holds(&named) {
            let key = QuarantineKey::Content(*blake3::hash(&bytes).as_bytes());
            if self.refusals.is_quarantined(&named, &key) {
                self.refusals.note_refused_again(&named);
                return Err(ReadError::Refused(SecurityRefusal {
                    path: named,
                    trust,
                    layer: TrustLayer::File,
                    reason: SecurityReason::Quarantined,
                }));
            }
            self.refusals.note_read(&named, &key);
        }
        Ok(bytes)
    }

    /// Returns the text of the file `rel` names under the root.
    ///
    /// # Errors
    ///
    /// As [`read`](Self::read), and [`ReadError::Io`] when it is not UTF-8.
    pub fn read_to_string(&self, rel: impl AsRef<Path>) -> Result<String, ReadError> {
        let bytes = self.read(rel)?;
        String::from_utf8(bytes).map_err(|error| {
            ReadError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, error))
        })
    }

    /// Reports a refusal of the file `path` and returns it as an error.
    fn refuse(
        &self,
        path: PathBuf,
        trust: Trust,
        reason: SecurityReason,
        key: Option<QuarantineKey>,
    ) -> ReadError {
        let refusal = SecurityRefusal {
            path,
            trust,
            layer: TrustLayer::File,
            reason,
        };
        self.refusals.refuse(refusal.clone(), key);
        ReadError::Refused(refusal)
    }
}

/// Whether `rel`, read as written, leaves the folder it is joined to: an
/// absolute path, or `..` past its start.
fn leaves(rel: &Path) -> bool {
    let mut depth = 0usize;
    for component in rel.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => return true,
            Component::CurDir => {}
            Component::ParentDir => match depth.checked_sub(1) {
                Some(up) => depth = up,
                None => return true,
            },
            Component::Normal(_) => depth += 1,
        }
    }
    false
}

/// Whether the file at `path` carries the system's download mark: Windows'
/// `Zone.Identifier` stream.
fn downloaded(path: &Path) -> bool {
    if cfg!(windows) {
        let mut mark = path.as_os_str().to_owned();
        mark.push(":Zone.Identifier");
        std::fs::metadata(PathBuf::from(mark)).is_ok()
    } else {
        false
    }
}
