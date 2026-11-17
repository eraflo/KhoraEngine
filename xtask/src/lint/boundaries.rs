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

//! Checks the crate boundaries one file shows: a lane never names the
//! `World`, an agent never reads `EngineContext.runtime`.

use super::code::{names_word, runtime_field_reads, strip_rust_comments};

/// Source folder of the lanes, whose files never name the `World`.
pub const LANES_SRC: &str = "crates/khora-lanes/src/";

/// Source folder of the agents, whose files never read the context's runtime.
pub const AGENTS_SRC: &str = "crates/khora-agents/src/";

/// Names each line of `text` naming `World` as a word outside comments, with
/// the line's code.
pub fn world_names(text: &str) -> Vec<(usize, String)> {
    strip_rust_comments(text)
        .lines()
        .enumerate()
        .filter(|(_, line)| names_word(line, "World"))
        .map(|(index, line)| (index + 1, line.trim().to_owned()))
        .collect()
}

/// Names each line of `text` reading the `runtime` field of an
/// `EngineContext` binding.
pub fn runtime_reads(text: &str) -> Vec<(usize, String)> {
    runtime_field_reads(&strip_rust_comments(text))
        .into_iter()
        .map(|line| {
            let message =
                "reads `EngineContext.runtime` directly; go through `resource` or `locked`";
            (line, message.to_owned())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_world_named_in_code_is_found_and_one_in_a_comment_is_not() {
        let text = "use a::World;\n// World\nlet w: RenderWorld;\n";
        assert_eq!(world_names(text), vec![(1, "use a::World;".to_owned())]);
    }

    #[test]
    fn a_runtime_read_through_a_context_parameter_is_found() {
        let text = "fn f(ctx: &EngineContext<'_>) {\n    let r = &ctx.runtime;\n}\n";
        let lines: Vec<usize> = runtime_reads(text)
            .into_iter()
            .map(|(line, _)| line)
            .collect();
        assert_eq!(lines, vec![2]);
    }
}
