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

//! The book's figures, held to what lets several share a page.
//!
//! A figure is an SVG under `docs/src/images/` inlined into a page with
//! `{{#include}}`. Inlined, its `<style>` is the page's: a rule it does not
//! scope to its own root id restyles every other figure there — the bug
//! that once drew one figure's code text in another's dark panel colour. A
//! blank line ends mdBook's HTML block and spills the rest of the SVG as
//! text; two figures sharing an id point a title or a marker at the wrong
//! one.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// What is wrong with one figure, in words a contributor can act on.
fn check_figure(source: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let Some(open) = root_tag(source) else {
        return vec!["no <svg> root".to_owned()];
    };
    let id = attribute(open, "id");
    match id {
        Some(id) if id.starts_with("fig-") => {}
        Some(id) => problems.push(format!("the root id `{id}` does not start with `fig-`")),
        None => problems.push("the <svg> root has no id".to_owned()),
    }
    if attribute(open, "role") != Some("img") {
        problems.push("the <svg> root is not `role=\"img\"`".to_owned());
    }
    match attribute(open, "aria-labelledby") {
        Some(labels) => {
            for label in labels.split_whitespace() {
                if !source.contains(&format!("id=\"{label}\"")) {
                    problems.push(format!(
                        "`aria-labelledby` names `{label}`, which is not there"
                    ));
                }
            }
        }
        None => problems.push("the <svg> root has no `aria-labelledby` title".to_owned()),
    }
    if source.lines().any(|line| line.trim().is_empty()) {
        problems.push("a blank line inside the SVG ends mdBook's HTML block".to_owned());
    }
    if let Some(id) = id {
        let own = format!("#{id}");
        for selector in style_selectors(source) {
            let scoped = selector == own
                || selector.starts_with(&format!("{own} "))
                || selector.starts_with(&format!("{own}."))
                || selector.starts_with(&format!("{own}:"));
            if !scoped {
                problems.push(format!(
                    "the style rule `{selector}` is not scoped to `{own}`"
                ));
            }
        }
    }
    problems
}

/// The opening `<svg …>` tag.
fn root_tag(source: &str) -> Option<&str> {
    let start = source.find("<svg")?;
    let end = source[start..].find('>')? + start;
    Some(&source[start..=end])
}

/// The value of `name="…"` in `tag`.
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let start = tag.find(&key)? + key.len();
    let end = tag[start..].find('"')? + start;
    Some(&tag[start..end])
}

/// Every selector of every rule in the figure's `<style>` blocks.
fn style_selectors(source: &str) -> Vec<String> {
    let mut selectors = Vec::new();
    let mut rest = source;
    while let Some(open) = rest.find("<style>") {
        let body_start = open + "<style>".len();
        let Some(close) = rest[body_start..].find("</style>") else {
            break;
        };
        let body = &rest[body_start..body_start + close];
        for rule in body.split('}') {
            if let Some((head, _)) = rule.split_once('{') {
                selectors.extend(
                    head.split(',')
                        .map(|selector| selector.trim().to_owned())
                        .filter(|selector| !selector.is_empty()),
                );
            }
        }
        rest = &rest[body_start + close..];
    }
    selectors
}

/// Every `id="…"` the figure declares.
fn ids(source: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = source;
    while let Some(at) = rest.find(" id=\"") {
        let start = at + " id=\"".len();
        let Some(len) = rest[start..].find('"') else {
            break;
        };
        found.push(&rest[start..start + len]);
        rest = &rest[start + len..];
    }
    found
}

fn figures(dir: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            figures(&path, into);
        } else if path.extension().is_some_and(|ext| ext == "svg") {
            into.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"<svg id="fig-a" class="kp-figure" role="img" aria-labelledby="kp-a-title">
  <title id="kp-a-title">A</title>
  <style>
    #fig-a { width: 100%; }
    #fig-a .kp-label, #fig-a .kp-note { fill: red; }
  </style>
</svg>"#;

    #[test]
    fn a_figure_scoping_every_rule_to_its_id_passes() {
        assert_eq!(check_figure(GOOD), Vec::<String>::new());
    }

    #[test]
    fn an_unscoped_style_rule_is_refused() {
        let leaky = GOOD.replace("#fig-a .kp-note", ".kp-note");
        let problems = check_figure(&leaky);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("`.kp-note`"), "{problems:?}");
    }

    #[test]
    fn a_rule_scoped_to_another_figure_is_refused() {
        let borrowed = GOOD.replace("#fig-a .kp-note", "#fig-b .kp-note");
        assert_eq!(check_figure(&borrowed).len(), 1);
    }

    #[test]
    fn a_blank_line_inside_a_figure_is_refused() {
        let spilled = GOOD.replace("  <style>", "\n  <style>");
        assert!(check_figure(&spilled)
            .iter()
            .any(|problem| problem.contains("blank line")));
    }

    #[test]
    fn a_figure_without_its_id_role_or_title_is_refused() {
        let bare = GOOD
            .replace(" id=\"fig-a\"", "")
            .replace(" role=\"img\"", "")
            .replace(
                "aria-labelledby=\"kp-a-title\"",
                "aria-labelledby=\"kp-gone\"",
            );
        let problems = check_figure(&bare);
        assert!(problems.iter().any(|p| p.contains("no id")), "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("role")), "{problems:?}");
        assert!(
            problems.iter().any(|p| p.contains("kp-gone")),
            "{problems:?}"
        );
    }

    /// **Every figure of the book keeps its styles to itself**, and no id is
    /// declared by two of them.
    #[test]
    fn every_figure_of_the_book_can_share_a_page() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/src/images");
        let mut paths = Vec::new();
        figures(&root, &mut paths);
        paths.sort();
        assert!(!paths.is_empty(), "no figure under {}", root.display());

        let mut problems = Vec::new();
        let mut owners: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for path in &paths {
            let name = path
                .strip_prefix(&root)
                .unwrap_or(path)
                .display()
                .to_string();
            let source = match std::fs::read_to_string(path) {
                Ok(source) => source.replace("\r\n", "\n"),
                Err(err) => {
                    problems.push(format!("{name}: cannot be read: {err}"));
                    continue;
                }
            };
            problems.extend(
                check_figure(&source)
                    .into_iter()
                    .map(|p| format!("{name}: {p}")),
            );
            for id in ids(&source) {
                owners.entry(id.to_owned()).or_default().push(name.clone());
            }
        }
        for (id, files) in owners {
            if files.len() > 1 {
                problems.push(format!("the id `{id}` is declared by {}", files.join(", ")));
            }
        }
        assert!(problems.is_empty(), "\n{}", problems.join("\n"));
    }
}
