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

//! The words a tool shows a refusal in: what refused it, and the trust it was read at.

use std::path::PathBuf;

use super::super::*;

fn refusal(layer: TrustLayer, reason: SecurityReason) -> SecurityRefusal {
    SecurityRefusal {
        path: PathBuf::from("assets/a.png"),
        trust: Trust::Project,
        layer,
        reason,
    }
}

/// **Each trust is named in a sentence's words.**
#[test]
fn each_trust_is_named_in_a_sentence() {
    for (trust, label) in [
        (Trust::Engine, "engine"),
        (Trust::Project, "trusted project"),
        (Trust::Restricted, "restricted project"),
        (Trust::Imported, "downloaded file"),
        (Trust::Save, "save"),
    ] {
        assert_eq!(trust.label(), label, "{trust:?}");
    }
}

/// **What refused a file is named by its layer**, the content's as a type check.
#[test]
fn what_refused_a_file_is_named_by_its_layer() {
    let too_large = || SecurityReason::TooLarge { size: 2, cap: 1 };
    for (layer, label) in [
        (TrustLayer::File, "file guard"),
        (TrustLayer::Decoder, "decoder"),
        (TrustLayer::Content, "type check"),
        (TrustLayer::Pack, "pack"),
    ] {
        assert_eq!(refusal(layer, too_large()).refused_by(), label, "{layer:?}");
    }
}

/// **A file whose bytes lie about its type was refused by the type check**,
/// whichever layer saw it.
#[test]
fn a_wrong_type_is_refused_by_the_type_check_at_any_layer() {
    for layer in [
        TrustLayer::File,
        TrustLayer::Decoder,
        TrustLayer::Content,
        TrustLayer::Pack,
    ] {
        let wrong = SecurityReason::WrongType {
            expected: "texture".to_owned(),
        };
        assert_eq!(
            refusal(layer, wrong).refused_by(),
            "type check",
            "{layer:?}"
        );
    }
}
