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

//! Script decoder: `.erg` bytes → [`ScriptModule`].
//!
//! Reads the text and lists what it imports. It does **not** compile: an
//! `import` brings in another file's declarations, so compiling needs the whole
//! reachable set and a decoder sees one file. Compiling here would produce a
//! program built against whatever happened to be loaded at the time.
//!
//! # A file that does not parse still decodes
//!
//! Deliberately. A syntax error is the author's to see and fix, and the way they
//! see it is by the editor reloading the file and reporting where. A decoder
//! that refused the bytes would leave nothing to report *against* — the asset
//! would simply be missing, and the message would be about a failed load rather
//! than about line 12. What is refused here is only what is not text at all.

use anyhow::{Context, Result};
use khora_core::asset::ScriptModule;

use crate::asset::{AssetDecoder, DecoderRegistration};

/// Decodes a `.erg` file into its source and its import list.
#[derive(Clone, Default)]
pub struct ScriptDecoder;

impl AssetDecoder<ScriptModule> for ScriptDecoder {
    fn load(
        &self,
        bytes: &[u8],
    ) -> Result<ScriptModule, Box<dyn std::error::Error + Send + Sync + 'static>> {
        let source =
            String::from_utf8(bytes.to_vec()).context("Ergon source is not valid UTF-8")?;
        let imports = imports_of(&source);
        Ok(ScriptModule::new(source, imports))
    }
}

/// The modules a source file imports, deduplicated, in source order.
///
/// Parsed rather than scanned for the word `import`: the string `"import"`
/// inside a literal or a comment is not one, and a dependency graph built from
/// a text search would load files nobody asked for — or, worse, miss the ones
/// they did.
///
/// A file that fails to parse yields what the parser recovered before it gave
/// up, which for imports is usually everything: they sit at the top, and the
/// parser recovers statement by statement.
pub fn imports_of(source: &str) -> Vec<String> {
    let lexed = khora_script::lex(source);
    let parsed = khora_script::parse(lexed.tokens);

    let mut seen = Vec::new();
    for import in &parsed.module.imports {
        if !seen.contains(&import.path) {
            seen.push(import.path.clone());
        }
    }
    seen
}

inventory::submit! {
    DecoderRegistration {
        type_name: "script",
        register: |svc| {
            svc.register_decoder::<ScriptModule>("script", ScriptDecoder);
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(source: &str) -> ScriptModule {
        ScriptDecoder.load(source.as_bytes()).expect("decodes")
    }

    #[test]
    fn a_module_keeps_its_source_verbatim() {
        let source = "fn void Main() { }\n";
        assert_eq!(decode(source).source, source);
    }

    #[test]
    fn imports_are_listed_in_source_order() {
        let module = decode(
            r#"import "combat/damage.erg";
               import "ai/steering.erg";
               fn void Main() { }"#,
        );
        assert_eq!(module.imports, vec!["combat/damage.erg", "ai/steering.erg"]);
    }

    /// A file that imports the same module twice depends on it once, and the
    /// asset system's dependency lists are deduplicated by contract.
    #[test]
    fn a_repeated_import_is_listed_once() {
        let module = decode(
            r#"import "shared.erg";
               import "shared.erg";
               fn void Main() { }"#,
        );
        assert_eq!(module.imports, vec!["shared.erg"]);
    }

    #[test]
    fn a_module_with_no_imports_is_a_leaf() {
        assert!(decode("fn void Main() { }").is_leaf());
    }

    /// **Why it parses rather than searching for the word.** A dependency graph
    /// built from a text scan would load a file nobody asked for.
    #[test]
    fn the_word_import_in_a_string_is_not_an_import() {
        let module = decode(r#"fn void Main() { Log("import \"ghost.erg\";"); }"#);
        assert!(module.is_leaf(), "got {:?}", module.imports);
    }

    #[test]
    fn the_word_import_in_a_comment_is_not_an_import() {
        let module = decode(
            r#"// import "ghost.erg";
               fn void Main() { }"#,
        );
        assert!(module.is_leaf(), "got {:?}", module.imports);
    }

    /// **A syntax error still decodes.** The author sees it because the editor
    /// reloaded the file and reported line 12 — which needs the file to have
    /// arrived at all.
    #[test]
    fn a_file_that_does_not_parse_still_decodes() {
        let module = decode(
            r#"import "real.erg";
               fn void ( ) { }"#,
        );
        assert_eq!(module.imports, vec!["real.erg"], "recovered what it could");
        assert!(module.source.contains("fn void ( )"), "and kept the text");
    }

    /// What is refused is only what is not text at all.
    #[test]
    fn bytes_that_are_not_utf8_are_refused() {
        assert!(ScriptDecoder.load(&[0xff, 0xfe, 0x00]).is_err());
    }
}
