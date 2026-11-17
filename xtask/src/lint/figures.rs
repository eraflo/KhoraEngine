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

//! Checks one figure of the book: an SVG under `docs/src/images/` that a page
//! inlines, so that its styles, ids and classes share the page with every
//! other figure on it.
//!
//! Inlined, a figure's `<style>` is the page's: a rule not scoped to the
//! figure's root id restyles every other figure there. A blank line ends the
//! CommonMark HTML block and shows the rest of the SVG as text. A class
//! without the `kp-` prefix can meet a class of the page's own theme.

use super::code::is_ident_char;

/// Folder of the book's figures, relative to the repository root.
pub const FIGURES_DIR: &str = "docs/src/images/";

/// Prefix of a figure's root id.
pub const FIGURE_ID_PREFIX: &str = "fig-";

/// Prefix of every class a figure's elements carry.
pub const FIGURE_CLASS_PREFIX: &str = "kp-";

/// One start tag of a figure, with the text of a `<style>` it opens.
struct Tag<'a> {
    /// The element's name: `svg`, `title`, `style`, …
    name: &'a str,
    /// The attributes, in source order, values unquoted.
    attributes: Vec<(&'a str, &'a str)>,
    /// Byte offset of the tag's `<` in the source.
    offset: usize,
    /// The raw text up to the closing tag, for `<style>` and `<script>`.
    body: Option<&'a str>,
}

impl<'a> Tag<'a> {
    /// Returns the value of the attribute `name`, if the tag has it.
    fn attribute(&self, name: &str) -> Option<&'a str> {
        self.attributes
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| *value)
    }
}

/// Names every rule `source`, one figure's SVG text, breaks, in words a
/// contributor can act on.
///
/// The rules: the root `<svg>` has an `id` starting `fig-`, `role="img"`
/// and an `aria-labelledby` whose ids all exist, one of them a `<title>`'s;
/// every selector of every `<style>` rule (inside `@media` too; `@keyframes`
/// steps are not selectors) starts with `#<that id>`, and no other at-rule
/// stands there; no line between `<svg` and `</svg>` is blank; every class
/// starts with `kp-`.
pub fn figure_problems(source: &str) -> Vec<String> {
    let tags = tags(source);
    let Some(root) = tags.first().filter(|tag| tag.name == "svg") else {
        return vec!["the file does not start with an `<svg>` root".to_owned()];
    };
    let mut problems = root_problems(root, &tags);
    if let Some(id) = root.attribute("id") {
        problems.extend(style_problems(id, &tags));
    }
    problems.extend(blank_line_problems(source, root.offset));
    problems.extend(class_problems(source, &tags));
    problems
}

/// Lists every `id` an element of `source` declares, in source order; ids
/// inside comments and `<style>` text are not elements'.
pub fn declared_ids(source: &str) -> Vec<String> {
    tags(source)
        .iter()
        .filter_map(|tag| tag.attribute("id"))
        .map(str::to_owned)
        .collect()
}

/// Names what is wrong with the root's id, role and accessible name.
fn root_problems(root: &Tag<'_>, tags: &[Tag<'_>]) -> Vec<String> {
    let mut problems = Vec::new();
    match root.attribute("id") {
        Some(id) if id.len() > FIGURE_ID_PREFIX.len() && id.starts_with(FIGURE_ID_PREFIX) => {}
        Some(id) => problems.push(format!(
            "the root id `{id}` does not start with `{FIGURE_ID_PREFIX}`"
        )),
        None => problems.push("the `<svg>` root has no `id`".to_owned()),
    }
    if root.attribute("role") != Some("img") {
        problems.push("the `<svg>` root is not `role=\"img\"`".to_owned());
    }
    let labels: Vec<&str> = root
        .attribute("aria-labelledby")
        .unwrap_or_default()
        .split_whitespace()
        .collect();
    if labels.is_empty() {
        problems.push("the `<svg>` root has no `aria-labelledby`".to_owned());
        return problems;
    }
    let declares = |label: &str, name: Option<&str>| {
        tags.iter().any(|tag| {
            tag.attribute("id") == Some(label) && name.is_none_or(|name| tag.name == name)
        })
    };
    for label in &labels {
        if !declares(label, None) {
            problems.push(format!(
                "`aria-labelledby` names `{label}`, which no element declares"
            ));
        }
    }
    if !labels.iter().any(|label| declares(label, Some("title"))) {
        problems.push("`aria-labelledby` names no `<title>`".to_owned());
    }
    problems
}

/// Names the selectors of the `<style>` blocks not scoped to `#id`, and the
/// at-rules a figure may not hold.
fn style_problems(id: &str, tags: &[Tag<'_>]) -> Vec<String> {
    let own = format!("#{id}");
    let mut problems = Vec::new();
    for body in tags
        .iter()
        .filter(|tag| tag.name == "style")
        .filter_map(|tag| tag.body)
    {
        let css = strip_css_comments(&body.replace("<![CDATA[", "").replace("]]>", ""));
        let mut selectors = Vec::new();
        read_rules(&css, &mut selectors, &mut problems);
        for selector in selectors {
            if !is_scoped(&selector, &own) {
                problems.push(format!(
                    "the style selector `{selector}` is not scoped to `{own}`"
                ));
            }
        }
    }
    problems
}

/// Tells whether `selector` starts with the id selector `own` as a whole name:
/// `#fig-a .kp-x`, `#fig-a:hover`, `#fig-a > g`, never `#fig-ab`.
fn is_scoped(selector: &str, own: &str) -> bool {
    selector
        .strip_prefix(own)
        .is_some_and(|rest| !rest.starts_with(|c: char| is_ident_char(c) || c == '-' || c == '\\'))
}

/// Collects every selector of the rules in `css` into `selectors`, and what is
/// no rule into `problems`.
///
/// It enters `@media` blocks and skips `@keyframes` ones; any other at-rule is
/// a problem.
fn read_rules(css: &str, selectors: &mut Vec<String>, problems: &mut Vec<String>) {
    let mut rest = css;
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            return;
        }
        let Some((stop, byte)) = outside_strings(rest)
            .into_iter()
            .find(|(_, byte)| matches!(byte, b'{' | b';'))
        else {
            problems.push(format!("the style text `{}` is not a rule", rest.trim()));
            return;
        };
        let prelude = rest[..stop].trim();
        if byte == b';' {
            if prelude.starts_with('@') {
                problems.push(format!("the at-rule `{prelude}` has no place in a figure"));
            } else if !prelude.is_empty() {
                problems.push(format!("the style text `{prelude}` is not a rule"));
            }
            rest = &rest[stop + 1..];
            continue;
        }
        let Some(close) = closing_brace(rest, stop) else {
            problems.push(format!("the style rule `{prelude}` is never closed"));
            return;
        };
        let block = &rest[stop + 1..close];
        if let Some(at_rule) = prelude.strip_prefix('@') {
            let name = at_rule
                .split(|c: char| c.is_whitespace() || c == '(')
                .next()
                .unwrap_or_default();
            if name == "media" {
                read_rules(block, selectors, problems);
            } else if !name.ends_with("keyframes") {
                problems.push(format!(
                    "the at-rule `@{name}` has no place in a figure: only `@media` and `@keyframes`"
                ));
            }
        } else {
            selectors.extend(
                split_selector_list(prelude)
                    .into_iter()
                    .filter(|selector| !selector.is_empty()),
            );
        }
        rest = &rest[close + 1..];
    }
}

/// Lists the bytes of `css` outside quoted strings, with their offsets.
fn outside_strings(css: &str) -> Vec<(usize, u8)> {
    let mut found = Vec::new();
    let mut quote = None;
    let mut escaped = false;
    for (at, byte) in css.bytes().enumerate() {
        match quote {
            Some(_) if escaped => escaped = false,
            Some(_) if byte == b'\\' => escaped = true,
            Some(open) if byte == open => quote = None,
            Some(_) => {}
            None if matches!(byte, b'"' | b'\'') => quote = Some(byte),
            None => found.push((at, byte)),
        }
    }
    found
}

/// Returns the offset of the `}` closing the `{` at `open` in `css`.
fn closing_brace(css: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (at, byte) in outside_strings(css)
        .into_iter()
        .filter(|(at, _)| *at >= open)
    {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(at);
                }
            }
            _ => {}
        }
    }
    None
}

/// Splits a selector list at its top-level commas, each part trimmed: the
/// commas inside `:is(…)` or `[…]` stay.
fn split_selector_list(prelude: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (at, byte) in outside_strings(prelude) {
        match byte {
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                parts.push(prelude[start..at].trim().to_owned());
                start = at + 1;
            }
            _ => {}
        }
    }
    parts.push(prelude[start..].trim().to_owned());
    parts
}

/// Replaces every `/* … */` comment of `css` with a space.
fn strip_css_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(open) = rest.find("/*") {
        out.push_str(&rest[..open]);
        out.push(' ');
        rest = rest[open + 2..]
            .find("*/")
            .map_or("", |close| &rest[open + 2 + close + 2..]);
    }
    out.push_str(rest);
    out
}

/// Names the blank lines between the root `<svg` at `root` and the last
/// `</svg>`.
fn blank_line_problems(source: &str, root: usize) -> Vec<String> {
    let end = source.rfind("</svg>").unwrap_or(source.len()).max(root);
    let first_line = line_of(source, root);
    source[root..end]
        .lines()
        .enumerate()
        .filter(|(_, line)| line.trim().is_empty())
        .map(|(index, _)| {
            format!(
                "line {} is blank: CommonMark ends the HTML block there",
                first_line + index
            )
        })
        .collect()
}

/// Names the classes of every element that lack the `kp-` prefix.
fn class_problems(source: &str, tags: &[Tag<'_>]) -> Vec<String> {
    let mut problems = Vec::new();
    for tag in tags {
        let Some(classes) = tag.attribute("class") else {
            continue;
        };
        for class in classes.split_whitespace() {
            let prefixed =
                class.len() > FIGURE_CLASS_PREFIX.len() && class.starts_with(FIGURE_CLASS_PREFIX);
            if !prefixed {
                problems.push(format!(
                    "line {}: the class `{class}` does not start with `{FIGURE_CLASS_PREFIX}`",
                    line_of(source, tag.offset)
                ));
            }
        }
    }
    problems
}

/// Returns the 1-based line of the byte offset `at` in `source`.
fn line_of(source: &str, at: usize) -> usize {
    source[..at].matches('\n').count() + 1
}

/// Lists the start tags of `source` in order, skipping comments, CDATA,
/// declarations, closing tags and the text of `<style>` and `<script>`.
fn tags(source: &str) -> Vec<Tag<'_>> {
    const SKIPPED: &[(&str, &str)] = &[
        ("<!--", "-->"),
        ("<![CDATA[", "]]>"),
        ("<!", ">"),
        ("<?", "?>"),
        ("</", ">"),
    ];
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(offset) = source[from..].find('<') {
        let at = from + offset;
        let after = &source[at..];
        if let Some((_, close)) = SKIPPED.iter().find(|(open, _)| after.starts_with(open)) {
            from = after
                .find(close)
                .map_or(source.len(), |end| at + end + close.len());
            continue;
        }
        let Some((mut tag, end, self_closing)) = start_tag(source, at) else {
            from = at + 1;
            continue;
        };
        from = end;
        if matches!(tag.name, "style" | "script") && !self_closing {
            let close = format!("</{}", tag.name);
            let body_end = source[from..]
                .find(&close)
                .map_or(source.len(), |end| from + end);
            tag.body = Some(&source[from..body_end]);
            from = body_end;
        }
        found.push(tag);
    }
    found
}

/// Parses the start tag whose `<` is at `at`: the tag, the offset after its
/// `>`, and whether it closes itself.
fn start_tag(source: &str, at: usize) -> Option<(Tag<'_>, usize, bool)> {
    let bytes = source.as_bytes();
    let is_name = |byte: u8| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':');
    let skip_space = |mut i: usize| {
        while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        i
    };
    let mut i = at + 1;
    while bytes.get(i).copied().is_some_and(is_name) {
        i += 1;
    }
    if i == at + 1 {
        return None;
    }
    let mut tag = Tag {
        name: &source[at + 1..i],
        attributes: Vec::new(),
        offset: at,
        body: None,
    };
    loop {
        i = skip_space(i);
        match bytes.get(i)? {
            b'>' => return Some((tag, i + 1, false)),
            b'/' if bytes.get(i + 1) == Some(&b'>') => return Some((tag, i + 2, true)),
            _ => {}
        }
        let key_start = i;
        while bytes
            .get(i)
            .is_some_and(|byte| !byte.is_ascii_whitespace() && !matches!(byte, b'=' | b'>' | b'/'))
        {
            i += 1;
        }
        if i == key_start {
            i += 1;
            continue;
        }
        let key = &source[key_start..i];
        i = skip_space(i);
        let mut value = "";
        if bytes.get(i) == Some(&b'=') {
            i = skip_space(i + 1);
            match bytes.get(i)? {
                quote @ (b'"' | b'\'') => {
                    let start = i + 1;
                    let end = source[start..].find(char::from(*quote))? + start;
                    value = &source[start..end];
                    i = end + 1;
                }
                _ => {
                    let start = i;
                    while bytes
                        .get(i)
                        .is_some_and(|byte| !byte.is_ascii_whitespace() && *byte != b'>')
                    {
                        i += 1;
                    }
                    value = &source[start..i];
                }
            }
        }
        tag.attributes.push((key, value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"<svg id="fig-a" class="kp-figure" viewBox="0 0 10 10" role="img"
     aria-labelledby="kp-a-title kp-a-desc">
  <title id="kp-a-title">A</title>
  <desc id="kp-a-desc">A figure.</desc>
  <style>
    /* scoped: .kp-note { } in a comment is no rule */
    #fig-a { width: 100%; font-family: 'Archivo', serif; }
    #fig-a .kp-label, #fig-a .kp-note { fill: red; }
    #fig-a:hover .kp-label, #fig-a > g, #fig-a.kp-wide { opacity: 1; }
    #fig-a :is(.kp-x, .kp-y) { fill: blue; }
    @media (prefers-reduced-motion: reduce) {
      #fig-a .kp-dot { animation: none; }
    }
    @keyframes kp-a-pulse { from { opacity: 0; } 50% { opacity: 1; } to { opacity: 0; } }
  </style>
  <!-- <g class="note"> in a comment is no element -->
  <g class="kp-dot kp-on"><text class="kp-label">x</text></g>
</svg>
"#;

    fn problems_with(from: &str, to: &str) -> Vec<String> {
        assert!(GOOD.contains(from), "`{from}` is not in the figure");
        figure_problems(&GOOD.replacen(from, to, 1))
    }

    fn mentions(problems: &[String], words: &str) -> bool {
        problems.iter().any(|problem| problem.contains(words))
    }

    #[test]
    fn a_figure_holding_every_rule_has_no_problem() {
        assert_eq!(figure_problems(GOOD), Vec::<String>::new());
    }

    #[test]
    fn a_file_not_opening_on_an_svg_root_is_refused() {
        let problems = figure_problems("<g class=\"kp-a\"/>");
        assert!(mentions(&problems, "`<svg>` root"), "{problems:?}");
    }

    #[test]
    fn a_root_id_without_the_fig_prefix_is_refused() {
        let problems = problems_with("id=\"fig-a\"", "id=\"a\"");
        assert!(
            mentions(&problems, "does not start with `fig-`"),
            "{problems:?}"
        );
    }

    #[test]
    fn a_root_without_an_id_is_refused() {
        let problems = problems_with(" id=\"fig-a\"", "");
        assert!(mentions(&problems, "has no `id`"), "{problems:?}");
    }

    #[test]
    fn a_root_not_role_img_is_refused() {
        let problems = problems_with("role=\"img\"", "role=\"figure\"");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(mentions(&problems, "role"), "{problems:?}");
    }

    #[test]
    fn a_label_naming_a_missing_element_is_refused() {
        let problems = problems_with("kp-a-desc\"", "kp-gone\"");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(mentions(&problems, "`kp-gone`"), "{problems:?}");
    }

    #[test]
    fn a_label_naming_no_title_is_refused() {
        let problems = problems_with("\"kp-a-title kp-a-desc\"", "\"kp-a-desc\"");
        assert_eq!(
            problems,
            vec!["`aria-labelledby` names no `<title>`".to_owned()]
        );
    }

    #[test]
    fn a_root_without_a_label_is_refused() {
        let problems = problems_with("aria-labelledby=\"kp-a-title kp-a-desc\"", "");
        assert!(mentions(&problems, "no `aria-labelledby`"), "{problems:?}");
    }

    #[test]
    fn an_unscoped_selector_in_a_list_is_refused() {
        let problems = problems_with("#fig-a .kp-note {", ".kp-note {");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(mentions(&problems, "`.kp-note`"), "{problems:?}");
    }

    #[test]
    fn a_selector_scoped_to_a_longer_id_is_refused() {
        let problems = problems_with("#fig-a > g", "#fig-ab > g");
        assert!(mentions(&problems, "`#fig-ab > g`"), "{problems:?}");
    }

    #[test]
    fn an_unscoped_selector_inside_media_is_refused() {
        let problems = problems_with("#fig-a .kp-dot { animation", ".kp-dot { animation");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(mentions(&problems, "`.kp-dot`"), "{problems:?}");
    }

    #[test]
    fn keyframe_steps_and_comments_are_not_selectors() {
        let only_keyframes = "<svg id=\"fig-k\" role=\"img\" aria-labelledby=\"t\">\n\
            <title id=\"t\">K</title>\n\
            <style>/* .x { } */ @keyframes kp-k { 0% { opacity: 0; } to { opacity: 1; } }</style>\n\
            </svg>";
        assert_eq!(figure_problems(only_keyframes), Vec::<String>::new());
    }

    #[test]
    fn an_at_rule_other_than_media_or_keyframes_is_refused() {
        let problems = problems_with(
            "@media (prefers-reduced-motion: reduce)",
            "@supports (x: y)",
        );
        assert!(mentions(&problems, "`@supports`"), "{problems:?}");
        let problems = problems_with("<style>", "<style>\n    @import url(x.css);");
        assert!(mentions(&problems, "`@import url(x.css)`"), "{problems:?}");
    }

    #[test]
    fn a_blank_line_inside_the_svg_is_refused_with_its_line() {
        let problems = problems_with("  <style>", "\n  <style>");
        assert_eq!(
            problems,
            vec!["line 5 is blank: CommonMark ends the HTML block there".to_owned()]
        );
    }

    #[test]
    fn a_line_of_spaces_is_blank() {
        let problems = problems_with("  <style>", "   \n  <style>");
        assert!(mentions(&problems, "line 5 is blank"), "{problems:?}");
    }

    #[test]
    fn a_class_without_the_kp_prefix_is_refused() {
        let problems = problems_with("class=\"kp-dot kp-on\"", "class=\"kp-dot on\"");
        assert_eq!(
            problems,
            vec!["line 17: the class `on` does not start with `kp-`".to_owned()]
        );
    }

    #[test]
    fn the_bare_prefix_is_no_class() {
        let problems = problems_with("class=\"kp-label\"", "class=\"kp-\"");
        assert!(mentions(&problems, "the class `kp-`"), "{problems:?}");
    }

    #[test]
    fn declared_ids_skip_comments_and_style_text() {
        let source = "<svg id=\"fig-a\"><!-- <g id=\"gone\"/> -->\
            <style>#x { }</style><title id=\"t\">x</title></svg>";
        assert_eq!(
            declared_ids(source),
            vec!["fig-a".to_owned(), "t".to_owned()]
        );
    }
}
