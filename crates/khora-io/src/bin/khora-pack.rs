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

//! `khora-pack <assets-root> <out-dir>` — packs a project's assets into the
//! release layout (`index.bin`, `data.pack`, `manifest.bin`) with
//! [`PackBuilder`], the packer the editor's Build Game uses: the same UUIDs,
//! resolved through the project's registry, and the same asset kinds.
//!
//! What it says goes through `log`, to a sink that writes to stderr — the
//! one place a `log::Log` may print, since it is where the log ends up.

use std::path::PathBuf;
use std::process::ExitCode;

use khora_io::asset::PackBuilder;

/// The log's one sink: stderr.
struct Stderr;

impl log::Log for Stderr {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            eprintln!("{}: {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

static SINK: Stderr = Stderr;

fn main() -> ExitCode {
    let _ = log::set_logger(&SINK).map(|()| log::set_max_level(log::LevelFilter::Info));
    let (root, out) = match arguments(std::env::args().skip(1)) {
        Ok(paths) => paths,
        Err(usage) => {
            log::error!("{usage}");
            return ExitCode::FAILURE;
        }
    };
    match PackBuilder::new(&root, &out).build() {
        Ok(packed) => {
            log::info!(
                "packed {} assets from '{}' into '{}' ({} bytes)",
                packed.asset_count,
                root.display(),
                out.display(),
                packed.pack_bytes()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            log::error!("packing '{}' failed: {error:#}", root.display());
            ExitCode::FAILURE
        }
    }
}

/// The assets root and the output directory, in that order — or what to
/// write instead.
fn arguments(mut args: impl Iterator<Item = String>) -> Result<(PathBuf, PathBuf), String> {
    const USAGE: &str = "usage: khora-pack <assets-root> <out-dir>";
    let root = args.next().ok_or(USAGE)?;
    let out = args.next().ok_or(USAGE)?;
    if args.next().is_some() {
        return Err(USAGE.to_owned());
    }
    Ok((PathBuf::from(root), PathBuf::from(out)))
}

#[cfg(test)]
mod tests {
    use super::arguments;

    fn args(list: &[&str]) -> impl Iterator<Item = String> {
        list.iter()
            .map(|arg| (*arg).to_owned())
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn the_root_and_the_output_are_read_in_order() {
        let (root, out) = arguments(args(&["game/assets", ".dist/game"])).expect("two paths");
        assert_eq!(root.to_str(), Some("game/assets"));
        assert_eq!(out.to_str(), Some(".dist/game"));
    }

    #[test]
    fn a_missing_or_an_extra_path_says_how_to_call_it() {
        for list in [&[][..], &["only-one"], &["a", "b", "c"]] {
            let refused = arguments(args(list)).expect_err("not two paths");
            assert!(refused.contains("usage: khora-pack"), "{refused}");
        }
    }
}
