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

//! **A packed game's scripts are compiled from its pack.**
//! - Its modules are the pack's `Script` entries under `scripts/`, read through
//!   the reader checked against the signed manifest.
//! - An entry whose bytes changed after the build, or one over its size limit,
//!   is refused, reported, and not compiled.
//! - Nothing outside the pack is read: a root other than the pack's compiles
//!   nothing.
//!
//! Every folder here is a temporary one.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use khora_core::asset::{AssetSource, AssetUUID, CompressionKind};
use khora_core::problems::{ProblemSource, Problems};
use khora_core::Runtime;
use khora_io::asset::signing::{PackSigner, SignedManifest};
use khora_io::asset::{
    AssetIndex, AssetService, PackBuilder, PackLoader, PackOutput, PACK_HEADER_SIZE,
};
use khora_io::script::project_compiler::ProjectCompiler;
use khora_io::script::sources::pack_modules;
use khora_io::trust::{FileKind, Limits, Refusals, SecurityReason, Trust};
use khora_script::toolchain::{CompileRequest, Recompiled, ScriptCompiler};
use khora_telemetry::MetricsRegistry;

/// A behaviour importing [`HELPER`].
const COUNTER: &str = "import \"lib/helper.erg\";
behavior Counter {
    int older = 0;
    void Update(float dt) { older += Step(); }
}";

/// A module [`COUNTER`] imports.
const HELPER: &str = "fn int Step() { return 1; }";

/// A module importing nothing.
const GUARD: &str = "fn void Main() { }";

/// A project's files written under `<dir>/project/assets/`, packed with
/// `compression` into `<dir>/exe/`, signed by a key kept in `<dir>/signing/`.
fn pack(files: &[(&str, &[u8])], compression: CompressionKind) -> (tempfile::TempDir, PackOutput) {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let assets = dir.path().join("project").join("assets");
    for (rel, bytes) in files {
        let path = assets.join(rel);
        std::fs::create_dir_all(path.parent().expect("a parent folder")).expect("the folders");
        std::fs::write(path, bytes).expect("the file is written");
    }
    let signer = PackSigner::load_or_create(&dir.path().join("signing"))
        .expect("a signing key in the temporary folder");
    let exe = dir.path().join("exe");
    let out = PackBuilder::new(&assets, &exe)
        .with_compression(compression)
        .with_signer(&signer)
        .build()
        .expect("the project packs");
    (dir, out)
}

/// The packed game `out` as its runtime opens it: verified with its key, its
/// decoders registered, reading at `trust` and refusing to `refusals`.
fn service_over(out: &PackOutput, trust: Trust, refusals: &Arc<Refusals>) -> AssetService {
    let dir = (out.index_bin.parent())
        .and_then(Path::parent)
        .expect("the pack's folder");
    let key = PackSigner::load_or_create(&dir.join("signing"))
        .expect("the key the pack was signed by")
        .public_key();
    let index = std::fs::read(&out.index_bin).expect("index.bin reads");
    let manifest_bin = out.manifest_bin.as_ref().expect("a manifest is written");
    let manifest = SignedManifest::decode(&std::fs::read(manifest_bin).expect("manifest.bin"))
        .expect("manifest.bin is a signed manifest")
        .verify(&key, &index)
        .expect("the manifest verifies");
    let loader = PackLoader::new(std::fs::File::open(&out.data_pack).expect("data.pack opens"))
        .expect("a valid pack header");
    let mut service = AssetService::new(
        &index,
        Box::new(loader),
        Arc::new(MetricsRegistry::new()),
        Some(manifest),
    )
    .expect("a service over the pack");
    service.set_refusals(Arc::clone(refusals));
    service.set_trust(trust);
    service.register_packed_decoders();
    service
}

/// A runtime holding the problems the refusals and the compiler report to.
fn reporting() -> (Runtime, Arc<Problems>, Arc<Refusals>) {
    let problems = Arc::new(Problems::default());
    let refusals = Arc::new(Refusals::new(Some(Arc::clone(&problems)), None));
    let mut runtime = Runtime::default();
    runtime.resources.insert(Arc::clone(&problems));
    runtime.resources.insert(Arc::clone(&refusals));
    (runtime, problems, refusals)
}

/// The root a packed game's scripts are filed under: its `data.pack`.
fn root_of(out: &PackOutput) -> PathBuf {
    out.data_pack.clone()
}

/// A request for every module the pack holds, filed under `root`.
fn every_module(service: &AssetService, root: PathBuf) -> CompileRequest {
    CompileRequest::new(root, pack_modules(service.index()).into_keys())
}

/// The modules `compiled` holds a program for.
fn programs(compiled: &Recompiled) -> BTreeSet<String> {
    compiled
        .reloads
        .iter()
        .filter(|reload| reload.program.is_some())
        .map(|reload| reload.module.clone())
        .collect()
}

fn set(modules: &[&str]) -> BTreeSet<String> {
    modules.iter().map(|module| (*module).to_owned()).collect()
}

/// Flips the first byte `rel`'s entry holds in `data.pack`.
fn tamper(out: &PackOutput, rel: &str) {
    let index = AssetIndex::new(&std::fs::read(&out.index_bin).expect("index.bin reads"))
        .expect("index.bin decodes");
    let offset = match index
        .get_metadata(&AssetUUID::new_v5(rel))
        .and_then(|metadata| metadata.default_source())
    {
        // Counted from the end of the header.
        Some(AssetSource::Packed { offset, .. }) => (PACK_HEADER_SIZE + *offset) as usize,
        other => panic!("{rel} is packed, not {other:?}"),
    };
    let mut pack = std::fs::read(&out.data_pack).expect("data.pack reads");
    pack[offset] ^= 0xFF;
    std::fs::write(&out.data_pack, pack).expect("data.pack is rewritten");
}

/// Asserts that `rel`'s entry is packed LZ4-compressed.
fn assert_compressed(out: &PackOutput, rel: &str) {
    let index = AssetIndex::new(&std::fs::read(&out.index_bin).expect("index.bin reads"))
        .expect("index.bin decodes");
    let compression = match index
        .get_metadata(&AssetUUID::new_v5(rel))
        .and_then(|metadata| metadata.default_source())
    {
        Some(AssetSource::Packed { compression, .. }) => *compression,
        other => panic!("{rel} is packed, not {other:?}"),
    };
    assert_eq!(
        compression,
        CompressionKind::Lz4,
        "{rel} is packed compressed"
    );
}

/// **A pack's modules are its scripts, known by their path under `scripts/`.**
///
/// Matched by the path the build packed them from: a `.erg` outside
/// `scripts/` and a file of another type are no module.
#[test]
fn a_packs_modules_are_its_scripts_under_the_script_folder() {
    let (_dir, out) = pack(
        &[
            ("scripts/counter.erg", COUNTER.as_bytes()),
            ("scripts/lib/helper.erg", HELPER.as_bytes()),
            ("props/lantern.erg", GUARD.as_bytes()),
            ("scenes/a.kscene", b"scene bytes"),
        ],
        CompressionKind::None,
    );
    let (_runtime, _problems, refusals) = reporting();
    let service = service_over(&out, Trust::Project, &refusals);

    let modules: BTreeSet<String> = pack_modules(service.index()).into_keys().collect();

    assert_eq!(modules, set(&["counter.erg", "lib/helper.erg"]));
}

/// **A packed game's modules compile from its pack**, importers and imports
/// alike, with nothing of them on disk.
#[test]
fn a_packed_module_compiles_from_the_pack() {
    let (dir, out) = pack(
        &[
            ("scripts/counter.erg", COUNTER.as_bytes()),
            ("scripts/lib/helper.erg", HELPER.as_bytes()),
        ],
        CompressionKind::None,
    );
    std::fs::remove_dir_all(dir.path().join("project")).expect("the project is taken away");
    let (runtime, problems, refusals) = reporting();
    let service = service_over(&out, Trust::Project, &refusals);
    let compiler = ProjectCompiler::for_pack(&runtime, root_of(&out), service.reader());

    let compiled = compiler.compile(&every_module(&service, root_of(&out)));

    assert_eq!(
        programs(&compiled),
        set(&["counter.erg", "lib/helper.erg"]),
        "problems: {:?}",
        problems.all()
    );
    assert!(
        compiled
            .declarations
            .iter()
            .any(|declared| declared.module == "counter.erg"
                && declared.components.iter().any(|c| c.name == "Counter")),
        "the counter's module declares `Counter`: {:?}",
        compiled.declarations
    );
}

/// **A pack entry whose bytes changed is refused and not compiled.**
///
/// One byte of `guard.erg` in `data.pack` changed after the build: its bytes
/// are not those the manifest was signed over. It is refused, as a tampered
/// pack entry, to the run's refusals and problems, and no program comes of
/// it; the untouched modules compile.
#[test]
fn a_pack_entry_whose_bytes_changed_is_refused_and_not_compiled() {
    let (_dir, out) = pack(
        &[
            ("scripts/counter.erg", COUNTER.as_bytes()),
            ("scripts/lib/helper.erg", HELPER.as_bytes()),
            ("scripts/guard.erg", GUARD.as_bytes()),
        ],
        CompressionKind::None,
    );
    tamper(&out, "scripts/guard.erg");
    let (runtime, problems, refusals) = reporting();
    let service = service_over(&out, Trust::Project, &refusals);
    let compiler = ProjectCompiler::for_pack(&runtime, root_of(&out), service.reader());

    let compiled = compiler.compile(&every_module(&service, root_of(&out)));

    assert_eq!(
        programs(&compiled),
        set(&["counter.erg", "lib/helper.erg"]),
        "the tampered module compiled, or an untouched one did not"
    );
    let refused: Vec<_> = refusals
        .quarantined()
        .into_iter()
        .filter(|refusal| refusal.path.ends_with("guard.erg"))
        .collect();
    assert_eq!(
        refused.iter().map(|r| r.reason.clone()).collect::<Vec<_>>(),
        vec![SecurityReason::Tampered],
        "the tampered entry is refused, once: {:?}",
        refusals.quarantined()
    );
    assert!(
        problems
            .of_source(&ProblemSource::Security)
            .iter()
            .any(|problem| problem
                .module
                .as_deref()
                .is_some_and(|m| m.ends_with("guard.erg"))),
        "the refusal is reported: {:?}",
        problems.all()
    );
}

/// **A packed script over its size limit is refused**, as the script decoder
/// refuses one: one byte over the `Text` limit at the reader's trust. It is
/// reported and not compiled.
#[test]
fn a_packed_script_over_its_size_limit_is_refused() {
    let cap = Limits::of(FileKind::Text, Trust::Restricted).max_bytes;
    let mut huge = b"fn void Main() { }\n//".to_vec();
    huge.resize(cap as usize + 1, b'a');
    let (_dir, out) = pack(
        &[
            ("scripts/huge.erg", huge.as_slice()),
            ("scripts/guard.erg", GUARD.as_bytes()),
        ],
        CompressionKind::Lz4,
    );
    let (runtime, problems, refusals) = reporting();
    let service = service_over(&out, Trust::Restricted, &refusals);
    let compiler = ProjectCompiler::for_pack(&runtime, root_of(&out), service.reader());

    let compiled = compiler.compile(&every_module(&service, root_of(&out)));

    assert_eq!(programs(&compiled), set(&["guard.erg"]));
    let refused: Vec<_> = refusals
        .quarantined()
        .into_iter()
        .filter(|refusal| refusal.path.ends_with("huge.erg"))
        .map(|refusal| refusal.reason)
        .collect();
    assert_eq!(
        refused,
        vec![SecurityReason::TooLarge { size: cap + 1, cap }],
        "the script over its limit is refused, once"
    );
    assert!(
        !problems.of_source(&ProblemSource::Security).is_empty(),
        "the refusal is reported"
    );
}

/// **A pack compiler reads nothing outside its pack.**
///
/// A `scripts/guard.erg` left on disk beside the pack, filed under that
/// folder's root, compiles to nothing: the module is withdrawn.
#[test]
fn a_pack_compiler_compiles_nothing_filed_under_another_root() {
    let (_dir, out) = pack(
        &[("scripts/guard.erg", GUARD.as_bytes())],
        CompressionKind::None,
    );
    let beside = out
        .data_pack
        .parent()
        .expect("the binary's folder")
        .join("assets");
    let planted = beside.join("scripts").join("planted.erg");
    std::fs::create_dir_all(planted.parent().expect("a folder")).expect("the folder");
    std::fs::write(&planted, GUARD).expect("the planted script");
    let (runtime, _problems, refusals) = reporting();
    let service = service_over(&out, Trust::Project, &refusals);
    let compiler = ProjectCompiler::for_pack(&runtime, root_of(&out), service.reader());

    let compiled = compiler.compile(&CompileRequest::new(
        beside,
        ["planted.erg".to_owned(), "guard.erg".to_owned()],
    ));

    assert!(
        programs(&compiled).is_empty(),
        "a script outside the pack compiled: {:?}",
        programs(&compiled)
    );
}

/// **A compressed pack entry whose bytes changed is refused as tampered.**
///
/// A release build compresses its pack. One byte of `guard.erg`'s compressed
/// bytes in `data.pack` changed after the build: like an uncompressed entry,
/// it is refused as a tampered pack entry, reported to the run's refusals and
/// problems, and not compiled.
#[test]
fn a_compressed_pack_entry_whose_bytes_changed_is_refused_as_tampered() {
    // Long enough for LZ4 to shrink it, so it is packed compressed.
    let guard = format!("{GUARD}\n//{}", "a".repeat(512));
    let (_dir, out) = pack(
        &[
            ("scripts/counter.erg", COUNTER.as_bytes()),
            ("scripts/lib/helper.erg", HELPER.as_bytes()),
            ("scripts/guard.erg", guard.as_bytes()),
        ],
        CompressionKind::Lz4,
    );
    assert_compressed(&out, "scripts/guard.erg");
    tamper(&out, "scripts/guard.erg");
    let (runtime, problems, refusals) = reporting();
    let service = service_over(&out, Trust::Project, &refusals);
    let compiler = ProjectCompiler::for_pack(&runtime, root_of(&out), service.reader());

    let compiled = compiler.compile(&every_module(&service, root_of(&out)));

    assert_eq!(
        programs(&compiled),
        set(&["counter.erg", "lib/helper.erg"]),
        "the tampered module compiled, or an untouched one did not"
    );
    let refused: Vec<_> = refusals
        .quarantined()
        .into_iter()
        .filter(|refusal| refusal.path.ends_with("guard.erg"))
        .map(|refusal| refusal.reason)
        .collect();
    assert_eq!(
        refused,
        vec![SecurityReason::Tampered],
        "the tampered compressed entry is not refused as tampered: {:?}; problems: {:?}",
        refusals.quarantined(),
        problems.all()
    );
}
