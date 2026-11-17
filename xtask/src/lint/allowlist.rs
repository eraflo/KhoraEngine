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

//! Reads the ratchet allowlist, `xtask/tests/layout_allow.txt`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

/// Every check, as named in the allowlist (each is its test's name).
pub const RATCHETED_CHECKS: &[&str] = &[
    "no_file_and_folder_module_pair",
    "no_orphan_rust_files",
    "test_files_follow_convention",
    "non_test_code_under_700_lines",
    "wgsl_only_in_shader_tree",
    "no_tracked_ignored_files",
    "relative_links_resolve",
    "cited_source_paths_exist",
    "no_duplicate_asset_files",
    "one_spelling_of_util",
    "no_generic_module_file_names",
    "docs_pages_are_kebab_case",
    "no_stuttering_file_names",
    "retired_file_names",
    "constants_come_before_the_code",
    "private_helpers_come_after_the_primary_type_impls",
    "each_file_holds_one_primary_type",
    "functions_under_200_lines",
    "doc_summary_is_short",
    "inline_comments_are_short",
    "a_type_and_its_impls_are_contiguous",
    "comments_tell_no_history",
    "fn_summaries_start_with_a_verb",
    "every_public_fn_is_named_by_a_test",
];

/// Words one of which an allowlist reason must contain: the batch that removes the entry.
///
/// `Deliberate` marks an entry kept on purpose; its reason says why.
pub const BATCH_WORDS: &[&str] = &[
    "B0",
    "B1",
    "B2",
    "B3",
    "B4",
    "B5",
    "B6",
    "P7",
    "Ergon",
    "Deliberate",
];

/// Repository-relative path of the ratchet allowlist.
pub const ALLOWLIST: &str = "xtask/tests/layout_allow.txt";

/// Violations of one check: repo-relative key → what is wrong there.
pub type Violations = BTreeMap<String, BTreeSet<String>>;

/// Records `detail` as wrong at `key`.
pub fn add(violations: &mut Violations, key: String, detail: String) {
    violations.entry(key).or_default().insert(detail);
}

/// Reads the allowlist under `root`: its entries per check, and the problems
/// found in the file.
pub fn read_allowlist(root: &Path) -> (BTreeMap<String, BTreeSet<String>>, Vec<String>) {
    let mut entries: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut problems = Vec::new();
    let Ok(text) = fs::read_to_string(root.join(ALLOWLIST)) else {
        return (entries, problems);
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    for (index, raw) in text.lines().enumerate() {
        let (line, reason) = raw.split_once('#').unwrap_or((raw, ""));
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [check, path] = fields.as_slice() else {
            problems.push(format!(
                "{ALLOWLIST}:{}: expected `<check-name> <path>  # reason`, got `{raw}`",
                index + 1
            ));
            continue;
        };
        if !RATCHETED_CHECKS.contains(check) {
            problems.push(format!(
                "{ALLOWLIST}:{}: `{check}` is not a layout check",
                index + 1
            ));
            continue;
        }
        let names_batch = reason
            .split(|ch: char| !ch.is_alphanumeric())
            .any(|word| BATCH_WORDS.contains(&word));
        if !names_batch {
            problems.push(format!(
                "{ALLOWLIST}:{}: `{path}` needs a `# reason` naming the batch that removes it \
                 (one of {BATCH_WORDS:?})",
                index + 1
            ));
        }
        if !entries
            .entry((*check).to_owned())
            .or_default()
            .insert((*path).to_owned())
        {
            problems.push(format!(
                "{ALLOWLIST}:{}: `{path}` is listed twice",
                index + 1
            ));
        }
    }
    (entries, problems)
}
