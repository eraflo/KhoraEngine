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

//! Tests of the asset service, by topic; the fixtures they share live here:
//! the mock IOs, the counting test container and its index, the note decoder.

mod announcements;
mod loading;
mod reader;
mod sub_assets;

use super::*;
use crate::asset::IndexBuilder;
use khora_core::asset::AssetSource;
use std::fs;
use tempfile::tempdir;

/// Mock IO returning bytes from an in-memory map keyed by relative path.
///
/// Covers invalidate and reindex without the real decoders' format-specific bytes.
struct MockIo {
    files: HashMap<std::path::PathBuf, Vec<u8>>,
}
impl AssetIo for MockIo {
    fn load_bytes(&self, source: &AssetSource) -> Result<Vec<u8>> {
        match source {
            AssetSource::Path(rel) => self
                .files
                .get(rel)
                .cloned()
                .ok_or_else(|| anyhow!("not in mock: {:?}", rel)),
            AssetSource::Packed { .. } => Err(anyhow!("mock doesn't support Packed")),
            // A sub-asset has no bytes of its own: the service reads its
            // container's source instead and never hands a `Sub` to the IO.
            AssetSource::Sub { .. } => {
                panic!("the service handed a Sub source to its AssetIo: {source:?}")
            }
        }
    }
}

use crate::asset::AssetContainer;
use khora_core::asset::AssetMetadata;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Index type of the test container files.
const CONTAINER_TYPE: &str = "ksubtest";

/// What a test container extracts as a `"mesh"`.
#[derive(Debug)]
struct TestMesh {
    container: AssetUUID,
    label: String,
    source_path: std::path::PathBuf,
    text: String,
}
impl Asset for TestMesh {}

/// What a test container extracts as a `"material"`.
#[derive(Debug)]
struct TestMaterial {
    label: String,
}
impl Asset for TestMaterial {}

/// What the test container's `open` produces.
struct OpenedTestContainer {
    text: String,
    source_path: std::path::PathBuf,
}

/// A container that counts how many times it was opened.
struct CountingContainer {
    opens: Arc<AtomicUsize>,
}

impl AssetContainer for CountingContainer {
    fn open(&self, bytes: &[u8], source_path: &Path) -> Result<Arc<dyn Any + Send + Sync>> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        Ok(Arc::new(OpenedTestContainer {
            text: String::from_utf8(bytes.to_vec())?,
            source_path: source_path.to_path_buf(),
        }))
    }

    fn extract(
        &self,
        opened: &(dyn Any + Send + Sync),
        container: AssetUUID,
        label: &str,
        type_name: &str,
    ) -> Result<Box<dyn Any + Send>> {
        let opened = opened
            .downcast_ref::<OpenedTestContainer>()
            .ok_or_else(|| anyhow!("not an opened test container"))?;
        match type_name {
            "mesh" => Ok(Box::new(TestMesh {
                container,
                label: label.to_string(),
                source_path: opened.source_path.clone(),
                text: opened.text.clone(),
            })),
            "material" => Ok(Box::new(TestMaterial {
                label: label.to_string(),
            })),
            other => Err(anyhow!("the test container holds no '{other}'")),
        }
    }
}

const HERO_PATH: &str = "models/hero.ksubtest";
const HERO_BYTES: &[u8] = b"HERO-CONTAINER";
const VILLAIN_PATH: &str = "models/villain.ksubtest";
const VILLAIN_BYTES: &[u8] = b"VILLAIN-CONTAINER";

fn hero() -> AssetUUID {
    AssetUUID::new_v5(HERO_PATH)
}
fn villain() -> AssetUUID {
    AssetUUID::new_v5(VILLAIN_PATH)
}
// The service reads a sub's container and label from its `Sub` source, so
// these uuids need not be derived through `AssetUUID::sub_asset`.
fn hero_body() -> AssetUUID {
    AssetUUID::new_v5("test-sub/hero/mesh/Body")
}
fn hero_skin() -> AssetUUID {
    AssetUUID::new_v5("test-sub/hero/material/Skin")
}
fn villain_body() -> AssetUUID {
    AssetUUID::new_v5("test-sub/villain/mesh/Body")
}

fn file_entry(uuid: AssetUUID, rel: &str) -> AssetMetadata {
    let mut variants = HashMap::new();
    variants.insert(
        "default".to_string(),
        AssetSource::Path(std::path::PathBuf::from(rel)),
    );
    AssetMetadata {
        uuid,
        source_path: std::path::PathBuf::from(rel),
        asset_type_name: CONTAINER_TYPE.to_string(),
        dependencies: Vec::new(),
        variants,
        tags: Vec::new(),
    }
}

fn sub_entry(
    uuid: AssetUUID,
    container: AssetUUID,
    rel: &str,
    label: &str,
    type_name: &str,
) -> AssetMetadata {
    let mut variants = HashMap::new();
    variants.insert(
        "default".to_string(),
        AssetSource::Sub {
            container,
            label: label.to_string(),
        },
    );
    AssetMetadata {
        uuid,
        source_path: std::path::PathBuf::from(rel),
        asset_type_name: type_name.to_string(),
        dependencies: Vec::new(),
        variants,
        tags: Vec::new(),
    }
}

/// An index holding two container files and their subs, as the index
/// builder writes it.
fn container_index_bytes() -> Vec<u8> {
    let metadata = vec![
        file_entry(hero(), HERO_PATH),
        sub_entry(hero_body(), hero(), HERO_PATH, "mesh/Body", "mesh"),
        sub_entry(hero_skin(), hero(), HERO_PATH, "material/Skin", "material"),
        file_entry(villain(), VILLAIN_PATH),
        sub_entry(villain_body(), villain(), VILLAIN_PATH, "mesh/Body", "mesh"),
    ];
    bincode::serde::encode_to_vec(&metadata, bincode::config::standard()).unwrap()
}

fn container_files(hero_bytes: &[u8]) -> HashMap<std::path::PathBuf, Vec<u8>> {
    let mut files = HashMap::new();
    files.insert(std::path::PathBuf::from(HERO_PATH), hero_bytes.to_vec());
    files.insert(
        std::path::PathBuf::from(VILLAIN_PATH),
        VILLAIN_BYTES.to_vec(),
    );
    files
}

/// A service over the container index with a counting container
/// registered; returns the open counter.
fn container_service(
    hero_bytes: &[u8],
    manifest: Option<PackManifest>,
) -> (AssetService, Arc<AtomicUsize>) {
    let metrics = Arc::new(MetricsRegistry::new());
    let mut svc = AssetService::new(
        &container_index_bytes(),
        Box::new(MockIo {
            files: container_files(hero_bytes),
        }),
        metrics,
        manifest,
    )
    .unwrap();
    let opens = Arc::new(AtomicUsize::new(0));
    svc.register_container(
        CONTAINER_TYPE,
        CountingContainer {
            opens: opens.clone(),
        },
    );
    (svc, opens)
}

use crate::asset::{AssetChanged, AssetDecoder};
use std::sync::Mutex;

/// Mock IO over a file map the test keeps a handle to, so it can rewrite
/// a file's bytes after the service took the IO.
struct SharedIo {
    files: Arc<Mutex<HashMap<std::path::PathBuf, Vec<u8>>>>,
}
impl AssetIo for SharedIo {
    fn load_bytes(&self, source: &AssetSource) -> Result<Vec<u8>> {
        match source {
            AssetSource::Path(rel) => self
                .files
                .lock()
                .unwrap()
                .get(rel)
                .cloned()
                .ok_or_else(|| anyhow!("not in mock: {:?}", rel)),
            other => Err(anyhow!("the shared mock only serves paths: {other:?}")),
        }
    }
}

/// Index type of the test's plain (non-container) files.
const NOTE_TYPE: &str = "ktestnote";
const NOTE_PATH: &str = "notes/readme.ktestnote";

fn note() -> AssetUUID {
    AssetUUID::new_v5(NOTE_PATH)
}

/// What the note decoder produces: the file's text.
#[derive(Debug)]
struct TestNote {
    #[allow(dead_code)]
    text: String,
}
impl Asset for TestNote {}

struct NoteDecoder;
impl AssetDecoder<TestNote> for NoteDecoder {
    /// A test note is any bytes: the test reads them as they are.
    fn sniff(&self, _head: &[u8]) -> bool {
        true
    }
    fn load(
        &self,
        bytes: &[u8],
    ) -> std::result::Result<TestNote, Box<dyn std::error::Error + Send + Sync>> {
        Ok(TestNote {
            text: String::from_utf8(bytes.to_vec())?,
        })
    }
}

/// Returns a service over the container index and one plain note, reading `files`.
///
/// The counting container and the note decoder are registered.
fn shared_service(files: Arc<Mutex<HashMap<std::path::PathBuf, Vec<u8>>>>) -> AssetService {
    let mut note_entry = file_entry(note(), NOTE_PATH);
    note_entry.asset_type_name = NOTE_TYPE.to_string();
    let metadata = vec![
        file_entry(hero(), HERO_PATH),
        sub_entry(hero_body(), hero(), HERO_PATH, "mesh/Body", "mesh"),
        sub_entry(hero_skin(), hero(), HERO_PATH, "material/Skin", "material"),
        file_entry(villain(), VILLAIN_PATH),
        sub_entry(villain_body(), villain(), VILLAIN_PATH, "mesh/Body", "mesh"),
        note_entry,
    ];
    let index = bincode::serde::encode_to_vec(&metadata, bincode::config::standard()).unwrap();
    let mut svc = AssetService::new(
        &index,
        Box::new(SharedIo { files }),
        Arc::new(MetricsRegistry::new()),
        None,
    )
    .unwrap();
    svc.register_container(
        CONTAINER_TYPE,
        CountingContainer {
            opens: Arc::new(AtomicUsize::new(0)),
        },
    );
    svc.register_decoder::<TestNote>(NOTE_TYPE, NoteDecoder);
    svc
}

fn shared_files() -> Arc<Mutex<HashMap<std::path::PathBuf, Vec<u8>>>> {
    let mut files = container_files(HERO_BYTES);
    files.insert(std::path::PathBuf::from(NOTE_PATH), b"hello".to_vec());
    Arc::new(Mutex::new(files))
}

fn drained_set(svc: &AssetService) -> HashSet<AssetUUID> {
    svc.changes().drain().into_iter().map(|c| c.0).collect()
}

use crate::asset::{AssetReader, Decoded};

/// The value `decoded` holds, as an `A`; panics naming `what` otherwise.
fn decoded_as<A: Asset>(decoded: Decoded, what: &str) -> Box<A> {
    decoded
        .value
        .unwrap_or_else(|e| panic!("{what} decodes: {e}"))
        .downcast::<A>()
        .unwrap_or_else(|_| panic!("{what} decodes as a {}", std::any::type_name::<A>()))
}

/// The note decoder, counting how many times it decoded.
struct CountingNoteDecoder {
    decodes: Arc<AtomicUsize>,
}
impl AssetDecoder<TestNote> for CountingNoteDecoder {
    fn sniff(&self, head: &[u8]) -> bool {
        NoteDecoder.sniff(head)
    }
    fn load(
        &self,
        bytes: &[u8],
    ) -> std::result::Result<TestNote, Box<dyn std::error::Error + Send + Sync>> {
        self.decodes.fetch_add(1, Ordering::SeqCst);
        NoteDecoder.load(bytes)
    }
}

/// The test container, reading `textures/skin.png` besides its own file.
struct ReadingBesideContainer(CountingContainer);
const SKIN_KEY: &str = "textures/skin.png";
impl AssetContainer for ReadingBesideContainer {
    fn open(&self, bytes: &[u8], source_path: &Path) -> Result<Arc<dyn Any + Send + Sync>> {
        self.0.open(bytes, source_path)
    }
    fn extract(
        &self,
        opened: &(dyn Any + Send + Sync),
        container: AssetUUID,
        label: &str,
        type_name: &str,
    ) -> Result<Box<dyn Any + Send>> {
        self.0.extract(opened, container, label, type_name)
    }
    fn external_files(&self, _opened: &(dyn Any + Send + Sync)) -> Vec<String> {
        vec![SKIN_KEY.to_string()]
    }
}

const LATER_PATH: &str = "notes/later.ktestnote";

fn later() -> AssetUUID {
    AssetUUID::new_v5(LATER_PATH)
}

fn note_entry(path: &str) -> AssetMetadata {
    let mut entry = file_entry(AssetUUID::new_v5(path), path);
    entry.asset_type_name = NOTE_TYPE.to_string();
    entry
}

fn index_of(entries: Vec<AssetMetadata>) -> Vec<u8> {
    bincode::serde::encode_to_vec(&entries, bincode::config::standard()).unwrap()
}

fn note_type() -> TypeId {
    TypeId::of::<TestNote>()
}
