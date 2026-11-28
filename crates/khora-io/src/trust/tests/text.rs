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

//! What reads as text.

use super::*;

/// **Text is UTF-8 with no NUL**: a head cut in the middle of a character is
/// still text, since a head is the start of a longer file.
#[test]
fn text_is_utf8_without_a_nul() {
    assert!(looks_like_text(b"behavior Guard {}\n"));
    assert!(looks_like_text(
        "caf\u{e9} \u{2014} \u{fc}n\u{ef}code".as_bytes()
    ));
    assert!(looks_like_text(b""));
    assert!(!looks_like_text(b"fn\0main"));
    assert!(!looks_like_text(b"\xFF\xFEnot utf-8"));
    assert!(!looks_like_text(b"\x89PNG\r\n\x1a\n"));

    let cafe = "caf\u{e9}".as_bytes();
    assert!(
        looks_like_text(&cafe[..cafe.len() - 1]),
        "a head cut inside its last character"
    );
}
