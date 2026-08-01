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

//! Import paths.
//!
//! Every path is relative to the project's script root and stays inside it.
//! That is not tidiness: a script that could import from an absolute path, or
//! climb out with `..`, would read whatever the author of a downloaded asset
//! pointed it at. The same containment the file API gets, applied to code.
//!
//! Paths are also **normalised**, so `ai/steering.erg` and `ai//steering.erg`
//! are one module rather than two. Without that, a diamond of imports would
//! compile the same file twice and its declarations would collide with
//! themselves.

/// Why a path was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathError {
    /// Begins at a filesystem root or a drive letter.
    Absolute,
    /// Contains a `..` segment.
    Escapes,
    /// Empty, or nothing but separators.
    Empty,
    /// Does not end in `.erg`.
    WrongExtension,
}

impl PathError {
    /// A message naming the rule.
    pub fn message(self) -> &'static str {
        match self {
            Self::Absolute => "an import path cannot be absolute",
            Self::Escapes => "an import path cannot leave the script root",
            Self::Empty => "an import path cannot be empty",
            Self::WrongExtension => "an import path must end in `.erg`",
        }
    }

    /// The reasoning, which is what makes the rule land rather than annoy.
    pub fn note(self) -> &'static str {
        match self {
            Self::Absolute | Self::Escapes => {
                "paths are relative to the project's script root and stay inside it, so a script cannot reach code the project does not own"
            }
            Self::Empty => "write the path to a script, e.g. `import \"ai/steering.erg\";`",
            Self::WrongExtension => "Ergon modules are `.erg` files",
        }
    }
}

/// Checks and normalises an import path.
///
/// Returns the canonical form: forward slashes, no duplicate or trailing
/// separators, no `.` segments.
pub fn normalise(path: &str) -> Result<String, PathError> {
    if path.trim().is_empty() {
        return Err(PathError::Empty);
    }

    // Backslashes are accepted on input so a Windows author's habit does not
    // produce a mystery, but the canonical form is always forward slashes.
    let unified = path.replace('\\', "/");

    if unified.starts_with('/') || has_drive_letter(&unified) {
        return Err(PathError::Absolute);
    }

    let mut segments = Vec::new();
    for segment in unified.split('/') {
        match segment {
            "" | "." => continue,
            // Rejected outright rather than resolved. `a/../b` could be
            // collapsed safely, but allowing the syntax at all invites
            // `../../secrets` and makes the rule something you have to compute
            // rather than read.
            ".." => return Err(PathError::Escapes),
            other => segments.push(other),
        }
    }

    if segments.is_empty() {
        return Err(PathError::Empty);
    }

    let joined = segments.join("/");
    if !joined.ends_with(".erg") {
        return Err(PathError::WrongExtension);
    }
    Ok(joined)
}

/// Whether the path starts with a Windows drive specifier.
fn has_drive_letter(path: &str) -> bool {
    let mut chars = path.chars();
    matches!(
        (chars.next(), chars.next()),
        (Some(letter), Some(':')) if letter.is_ascii_alphabetic()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_path_passes_through() {
        assert_eq!(
            normalise("ai/steering.erg"),
            Ok("ai/steering.erg".to_owned())
        );
    }

    /// Redundant separators must collapse, or a diamond of imports would
    /// compile the same file twice and collide with itself.
    #[test]
    fn redundant_separators_collapse() {
        assert_eq!(
            normalise("ai//steering.erg"),
            Ok("ai/steering.erg".to_owned())
        );
        assert_eq!(
            normalise("./ai/./steering.erg"),
            Ok("ai/steering.erg".to_owned())
        );
    }

    /// Backslashes are accepted so a Windows habit does not produce a mystery,
    /// but they normalise away.
    #[test]
    fn backslashes_normalise_to_forward_slashes() {
        assert_eq!(
            normalise("ai\\steering.erg"),
            Ok("ai/steering.erg".to_owned())
        );
    }

    /// The containment rule. A script must not be able to reach code the
    /// project does not own.
    #[test]
    fn escaping_the_root_is_refused() {
        assert_eq!(normalise("../secrets.erg"), Err(PathError::Escapes));
        assert_eq!(normalise("ai/../../secrets.erg"), Err(PathError::Escapes));
        // Refused even where it would collapse safely: the rule should be
        // readable, not computed.
        assert_eq!(normalise("ai/../steering.erg"), Err(PathError::Escapes));
    }

    #[test]
    fn absolute_paths_are_refused() {
        assert_eq!(normalise("/etc/passwd.erg"), Err(PathError::Absolute));
        assert_eq!(normalise("C:/windows/x.erg"), Err(PathError::Absolute));
        assert_eq!(normalise("C:\\windows\\x.erg"), Err(PathError::Absolute));
    }

    #[test]
    fn an_empty_path_is_refused() {
        assert_eq!(normalise(""), Err(PathError::Empty));
        assert_eq!(normalise("   "), Err(PathError::Empty));
        // Nothing but `.` segments: syntactically a path, but it names nothing.
        assert_eq!(normalise("./."), Err(PathError::Empty));
    }

    /// A path of nothing but separators is reported as absolute rather than
    /// empty. Both are true; leading with the more specific one is what tells
    /// the author which rule they crossed.
    #[test]
    fn a_path_of_separators_is_reported_as_absolute() {
        assert_eq!(normalise("///"), Err(PathError::Absolute));
    }

    #[test]
    fn a_non_erg_file_is_refused() {
        assert_eq!(normalise("ai/steering.txt"), Err(PathError::WrongExtension));
        assert_eq!(normalise("ai/steering"), Err(PathError::WrongExtension));
    }

    /// Every refusal carries its reasoning; a rule stated bare reads as
    /// arbitrary.
    #[test]
    fn every_error_explains_itself() {
        for error in [
            PathError::Absolute,
            PathError::Escapes,
            PathError::Empty,
            PathError::WrongExtension,
        ] {
            assert!(!error.message().is_empty());
            assert!(!error.note().is_empty());
        }
    }
}
