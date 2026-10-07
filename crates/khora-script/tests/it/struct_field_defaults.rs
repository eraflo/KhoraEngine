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

//! A struct field's default, checked against the field's written type.

use khora_script::{check, lex, parse, Diagnostic};

/// The errors the checker reports for `source`, which must lex and parse.
fn errors(source: &str) -> Vec<Diagnostic> {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    check(&parsed.module)
        .diagnostics
        .into_iter()
        .filter(Diagnostic::is_error)
        .collect()
}

/// **One mistake, one message.** A field of an unknown type is reported once
/// whether or not it has a default — checking the default must not resolve
/// the written type a second time and report it again.
#[test]
fn an_unknown_field_type_is_reported_once_with_a_default() {
    for source in ["struct S { Foo x; }", "struct S { Foo x = 1; }"] {
        let found = errors(source);
        let unknown = found
            .iter()
            .filter(|d| d.message.contains("unknown type `Foo`"))
            .count();
        assert_eq!(unknown, 1, "`{source}`: {found:?}");
    }
}
