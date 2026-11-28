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

//! The runtime-stamp strategy of "Build Game": the project's pack, and the
//! pre-built runtime staged beside it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use khora_sdk::trust::Refusals;
use khora_sdk::PackSigner;

use super::{
    pack_and_sign, sanitize_binary_name, set_executable_bit, stamp_binary, BuildOutcome,
    BuildPreset, BuildStrategy, BuildTarget,
};

/// What a build by runtime stamp packs, where it stages it, and how.
pub(super) struct RuntimeStampBuild<'a> {
    /// The project's `assets/`, packed.
    pub(super) assets_root: &'a Path,
    /// The run's refusals, which the pack reports what it refuses to.
    pub(super) refusals: Arc<Refusals>,
    /// The folder the pack and the binary are written in.
    pub(super) output_dir: &'a Path,
    /// The project's name, which the binary is named after.
    pub(super) project_name: &'a str,
    /// The platform the binary is staged for.
    pub(super) target: BuildTarget,
    /// The preset whose compression the pack has.
    pub(super) preset: BuildPreset,
    /// What signs the pack; its public key is stamped into the binary.
    pub(super) signer: &'a PackSigner,
    /// The pre-built runtime binary staged.
    pub(super) runtime_src: &'a Path,
}

/// Builds by runtime stamp: packs `build.assets_root` into `build.output_dir` and stages `build.runtime_src` there.
///
/// The pack has the preset's compression and is signed by the signer, its
/// refusals reported to the build's; the binary is named after the project and
/// stamped with the signer's public key.
pub(super) fn build_with_runtime(build: RuntimeStampBuild<'_>) -> Result<BuildOutcome> {
    let pack_out = pack_and_sign(
        build.assets_root,
        build.refusals,
        build.output_dir,
        build.preset,
        build.signer,
    )?;
    let bin_dst = stage_runtime(
        build.runtime_src,
        build.output_dir,
        build.project_name,
        build.target,
    )?;
    stamp_binary(&bin_dst, build.signer)?;
    Ok(BuildOutcome {
        strategy: BuildStrategy::RuntimeStamp,
        output_dir: build.output_dir.to_path_buf(),
        binary_path: bin_dst,
        asset_count: pack_out.asset_count,
        pack_bytes: pack_out.pack_bytes(),
        left_out: pack_out.left_out,
        reclaimed: Vec::new(),
    })
}

/// Copies the pre-built runtime at `runtime_src` into `output_dir`, renamed
/// to the project name. Returns the absolute path of the staged binary.
fn stage_runtime(
    runtime_src: &Path,
    output_dir: &Path,
    project_name: &str,
    target: BuildTarget,
) -> Result<PathBuf> {
    let safe_name = sanitize_binary_name(project_name);
    let bin_filename = format!("{}{}", safe_name, target.exe_suffix());
    let bin_dst = output_dir.join(&bin_filename);
    std::fs::copy(runtime_src, &bin_dst).with_context(|| {
        format!(
            "Failed to copy runtime {} → {}",
            runtime_src.display(),
            bin_dst.display()
        )
    })?;
    set_executable_bit(&bin_dst);
    Ok(bin_dst)
}
