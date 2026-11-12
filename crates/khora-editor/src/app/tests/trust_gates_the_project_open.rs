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

//! **A project opens at the trust the user decided of it.** The editor opens
//! a project one way — the `--project` of the command line, a folder picked
//! by File > Open Project — and that way reads what the trust store holds of
//! it: a project never decided of is held, nothing of it filed, until the
//! user decides; one decided of opens at that trust. Trusting a project
//! opened restricted lifts its restricted mode.
//!
//! Every folder here is under the temp folder: no test reads or writes the
//! user's home folder.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::EditorState;
use khora_sdk::khora_core::asset::IndexRequest;
use khora_sdk::khora_core::event::Channel;
use khora_sdk::trust::{
    Layer, ProjectTrust, ProjectTrustState, QuarantineKey, Refusals, SecurityReason,
    SecurityRefusal, Trust, TrustStore,
};
use khora_sdk::{IndexKind, ReadinessHold};

use crate::app::project_open::{grant_trust, open_project, start_opening_project};
use crate::tests::project_index::IndexingEngine;

/// A folder under the temp folder, removed when dropped.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "khora-editor-trust-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos())
        ));
        std::fs::create_dir_all(&root).expect("the folder");
        Folder(root)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A project named `name` under `folder`, with its `project.json` and an
/// `assets/` folder.
fn project_in(folder: &Folder, name: &str) -> PathBuf {
    let root = folder.0.join(name);
    std::fs::create_dir_all(root.join("assets")).expect("the project folder");
    std::fs::write(
        root.join("project.json"),
        format!(r#"{{ "name": "{name}", "engine_version": "0.9.1" }}"#),
    )
    .expect("the project's `project.json`");
    root
}

/// The trust store's file under `folder`, as `~/.khora/trust.ron` would be.
fn store_in(folder: &Folder) -> PathBuf {
    folder.0.join("khora").join("trust.ron")
}

/// What the editor opens a project on: the engine's asset service and index
/// hub, the run's refusals, the trust of the project open (`Trusted` until
/// an open sets it); the editor's state and hold.
struct Editor {
    engine: IndexingEngine,
    refusals: Arc<Refusals>,
    trust: Arc<ProjectTrustState>,
    state: Arc<Mutex<EditorState>>,
    hold: Option<ReadinessHold>,
}

impl Editor {
    fn new() -> Self {
        let mut engine = IndexingEngine::new();
        let refusals = Arc::new(Refusals::new(None, None));
        engine.runtime.resources.insert(Arc::clone(&refusals));
        let trust = Arc::new(ProjectTrustState::new(ProjectTrust::Trusted));
        engine.runtime.resources.insert(Arc::clone(&trust));
        // What the film waits on: an open takes a hold on it.
        engine
            .runtime
            .resources
            .insert(Arc::new(khora_sdk::Readiness::new()));
        Self {
            engine,
            refusals,
            trust,
            state: Arc::new(Mutex::new(EditorState::default())),
            hold: None,
        }
    }

    /// The project at `root` opened as the editor opens one, `store` read.
    fn open(&mut self, root: &Path, store: &TrustStore) -> bool {
        open_project(
            root.to_path_buf(),
            store,
            &self.engine.runtime,
            &self.state,
            &mut self.hold,
        )
        .is_some()
    }

    /// The index requests filed so far, taken off the family's channel.
    fn filed(&self) -> Vec<IndexRequest> {
        self.engine
            .runtime
            .resources
            .get::<Channel<IndexRequest>>()
            .cloned()
            .expect("the runtime holds the index requests")
            .drain()
    }

    fn state(&self) -> std::sync::MutexGuard<'_, EditorState> {
        self.state.lock().expect("the editor state")
    }
}

/// Whether `requests` holds an `Open` of the project at `root`, however its
/// folder is spelled.
fn opens(requests: &[IndexRequest], root: &Path) -> bool {
    let root = std::fs::canonicalize(root).expect("the project exists");
    requests.iter().any(|request| {
        request.kind == IndexKind::Open
            && std::fs::canonicalize(&request.project_root).ok().as_ref() == Some(&root)
    })
}

/// **An unknown project asks for trust once.** Opened with nothing decided
/// of it, the project is held: nothing is filed, nothing of it is shown as
/// open, no hold is taken on the film, and the editor's state names it as
/// waiting for the user's decision. Once decided — here, restricted — the
/// same open opens it: its `Open` is filed, the trust of the project open is
/// the one decided, and nothing waits for a decision any more. A second
/// editor over the same store opens it at once.
#[test]
fn an_unknown_project_asks_for_trust_once() {
    let folder = Folder::new("asks-once");
    let root = project_in(&folder, "ember");
    let store = TrustStore::new(Some(store_in(&folder)));
    let mut editor = Editor::new();

    assert!(!editor.open(&root, &store), "the open is held");

    assert!(editor.filed().is_empty(), "nothing is filed");
    assert!(editor.hold.is_none(), "no hold is taken on the film");
    {
        let state = editor.state();
        assert_eq!(
            state.trust_prompt.as_deref(),
            Some(root.as_path()),
            "the editor's state names the project waiting for a decision"
        );
        assert_eq!(state.project_folder, None, "nothing of it is shown as open");
    }

    store.decide(&root, ProjectTrust::Restricted);
    assert!(editor.open(&root, &store), "decided, the project opens");

    assert!(opens(&editor.filed(), &root), "its `Open` is filed");
    assert_eq!(
        editor.trust.get(),
        ProjectTrust::Restricted,
        "the project open is at the trust decided"
    );
    assert_eq!(
        editor.state().trust_prompt,
        None,
        "nothing waits for a decision any more"
    );

    let mut second = Editor::new();
    assert!(
        second.open(&root, &TrustStore::new(Some(store_in(&folder)))),
        "the decision is remembered: a second editor does not ask again"
    );
    assert_eq!(second.state().trust_prompt, None);
}

/// **The open waits for a decision, and opens at the one given**: with no
/// decision, `start_opening_project` files nothing and names the project as
/// waiting; with one, it files the project's `Open` and sets the trust of
/// the project open to it — restricted, then trusted.
#[test]
fn a_project_opens_at_the_trust_decided() {
    let folder = Folder::new("opens-at-trust");
    let root = project_in(&folder, "lantern");
    let mut editor = Editor::new();

    let held = start_opening_project(
        root.clone(),
        None,
        &editor.engine.runtime,
        &editor.state,
        &mut editor.hold,
    );
    assert!(held.is_none(), "no decision: the open is held");
    assert!(editor.filed().is_empty(), "nothing is filed");
    assert_eq!(editor.state().trust_prompt.as_deref(), Some(root.as_path()));

    let restricted = start_opening_project(
        root.clone(),
        Some(ProjectTrust::Restricted),
        &editor.engine.runtime,
        &editor.state,
        &mut editor.hold,
    );
    assert!(restricted.is_some(), "decided: the project opens");
    assert!(opens(&editor.filed(), &root), "its `Open` is filed");
    assert_eq!(editor.trust.get(), ProjectTrust::Restricted);
    assert_eq!(editor.state().trust_prompt, None);

    let trusted = start_opening_project(
        root.clone(),
        Some(ProjectTrust::Trusted),
        &editor.engine.runtime,
        &editor.state,
        &mut editor.hold,
    );
    assert!(trusted.is_some());
    assert_eq!(editor.trust.get(), ProjectTrust::Trusted);
}

/// **File > Open Project goes through the same gate**: a second project
/// picked while one is open, never decided of, is held and named as waiting;
/// the project open stays as it was, at its trust.
#[test]
fn a_project_picked_by_file_open_asks_for_trust_too() {
    let folder = Folder::new("file-open");
    let first = project_in(&folder, "first");
    let picked = project_in(&folder, "picked");
    let store = TrustStore::new(Some(store_in(&folder)));
    store.decide(&first, ProjectTrust::Trusted);
    let mut editor = Editor::new();
    assert!(editor.open(&first, &store), "the first project opens");
    editor.filed();

    assert!(!editor.open(&picked, &store), "the picked project is held");

    assert!(editor.filed().is_empty(), "nothing of it is filed");
    let state = editor.state();
    assert_eq!(state.trust_prompt.as_deref(), Some(picked.as_path()));
    assert_eq!(
        state.project_folder.as_deref().map(PathBuf::from),
        Some(first.clone()),
        "the project open stays shown"
    );
    drop(state);
    assert_eq!(editor.trust.get(), ProjectTrust::Trusted);
}

/// **Granting trust lifts the restricted mode.** A project opened
/// restricted, two of its files refused at the strict trust — one too large,
/// one holding what its type does not — is trusted: the store says so, the
/// trust of the project open is `Trusted`, the file refused for its size is
/// released while the one refused for its content stays, and the project's
/// index is filed anew, so every reader is built at a trusted project's
/// trust.
#[test]
fn granting_trust_lifts_the_restricted_mode() {
    let folder = Folder::new("grant");
    let root = project_in(&folder, "ember");
    let store_path = store_in(&folder);
    let store = TrustStore::new(Some(store_path.clone()));
    store.decide(&root, ProjectTrust::Restricted);
    let mut editor = Editor::new();
    assert!(
        editor.open(&root, &store),
        "the premise: it opens restricted"
    );
    assert_eq!(editor.trust.get(), ProjectTrust::Restricted);
    editor.filed();

    let too_large = SecurityRefusal {
        path: root.join("assets").join("huge.png"),
        trust: Trust::Restricted,
        layer: Layer::File,
        reason: SecurityReason::TooLarge {
            size: 80 << 20,
            cap: 64 << 20,
        },
    };
    let wrong_type = SecurityRefusal {
        path: root.join("assets").join("liar.png"),
        trust: Trust::Restricted,
        layer: Layer::Decoder,
        reason: SecurityReason::WrongType {
            expected: "texture".to_owned(),
        },
    };
    editor.refusals.refuse(
        too_large.clone(),
        Some(QuarantineKey::Stamp {
            len: 80 << 20,
            modified: std::time::SystemTime::UNIX_EPOCH
                + std::time::Duration::from_secs(1_700_000_000),
        }),
    );
    editor
        .refusals
        .refuse(wrong_type.clone(), Some(QuarantineKey::Content([3; 32])));

    grant_trust(&editor.engine.runtime, &store, &root);

    assert_eq!(store.decision(&root), Some(ProjectTrust::Trusted));
    assert_eq!(
        TrustStore::new(Some(store_path)).decision(&root),
        Some(ProjectTrust::Trusted),
        "the decision is written to the store"
    );
    assert_eq!(
        editor.trust.get(),
        ProjectTrust::Trusted,
        "the project open is trusted"
    );
    assert_eq!(
        editor.refusals.quarantined(),
        vec![wrong_type],
        "the file refused for its size is released, the one refused for its content stays"
    );
    assert!(
        opens(&editor.filed(), &root),
        "the project's index is filed anew"
    );
}

/// **Trusting a project lifts its own restricted refusals, not another
/// project's.** The quarantine is the user's, shared by every project they
/// open: a file of another project, never trusted, refused over a restricted
/// limit, stays quarantined when this one is trusted — that other project
/// is still read at the strict limits, and its file was never cleared.
#[test]
fn trusting_a_project_keeps_another_projects_restricted_refusals() {
    let folder = Folder::new("grant-own");
    let root = project_in(&folder, "ember");
    let other = project_in(&folder, "other");
    let store = TrustStore::new(Some(store_in(&folder)));
    store.decide(&root, ProjectTrust::Restricted);
    store.decide(&other, ProjectTrust::Restricted);
    let mut editor = Editor::new();
    assert!(
        editor.open(&root, &store),
        "the premise: it opens restricted"
    );
    editor.filed();

    let others = SecurityRefusal {
        path: other.join("assets").join("wide.png"),
        trust: Trust::Restricted,
        layer: Layer::Decoder,
        reason: SecurityReason::OverLimit {
            what: "image width of 16384 pixels".to_owned(),
            limit: 8192,
        },
    };
    editor
        .refusals
        .refuse(others.clone(), Some(QuarantineKey::Content([7; 32])));

    grant_trust(&editor.engine.runtime, &store, &root);

    assert_eq!(
        store.decision(&other),
        Some(ProjectTrust::Restricted),
        "the premise: the other project is still restricted"
    );
    assert_eq!(
        editor.refusals.quarantined(),
        vec![others],
        "the other project's file stays quarantined"
    );
}

/// **A restricted project's shader overlays are never used while another
/// project opens.** A project opened restricted lands: its assets are
/// watched, its overlay on disk is not used. The user then opens another
/// project, trusted — in `before_frame`, File > Open Project or a trust
/// answer: the trust of the project open is the new one's at once. The
/// restricted project is unmounted only in `before_agents`, so the frame's
/// `PreExtract` shader pump, between the two, still watches its assets: the
/// restricted project's overlay must still not be used.
#[test]
fn a_restricted_projects_overlay_is_not_used_while_a_trusted_project_opens() {
    let folder = Folder::new("overlay-while-opening");
    let restricted = project_in(&folder, "restricted");
    let shaders = restricted.join("assets").join("shaders");
    std::fs::create_dir_all(&shaders).expect("the shader folder");
    std::fs::write(shaders.join("x.wgsl"), "// the restricted project's").expect("the overlay");
    let trusted = project_in(&folder, "trusted");
    let store = TrustStore::new(Some(store_in(&folder)));
    store.decide(&restricted, ProjectTrust::Restricted);
    store.decide(&trusted, ProjectTrust::Trusted);

    let mut editor = Editor::new();
    khora_sdk::scripts::install(&mut editor.engine.runtime);
    let pipelines = Arc::new(OverlayLog::default());
    let as_system: Arc<dyn PipelineSystem> = pipelines.clone();
    editor.engine.runtime.resources.insert(as_system);
    let device: Arc<dyn GraphicsDevice> = Arc::new(NoDevice);
    editor.engine.runtime.backends.insert(device);

    assert!(
        editor.open(&restricted, &store),
        "the premise: it opens restricted"
    );
    // Its index landed: the editor mounts it, its assets watched.
    let mut mounted = None;
    crate::project_scripts::follow_open_project(
        &editor.engine.runtime,
        &mut mounted,
        Some(&restricted),
    );
    std::thread::sleep(std::time::Duration::from_millis(150));
    for _ in 0..5 {
        shader_pump(&editor.engine.runtime);
    }
    assert!(
        pipelines.set().is_empty(),
        "the premise: the restricted project's overlay is not used: {:?}",
        pipelines.set()
    );

    assert!(
        editor.open(&trusted, &store),
        "the trusted project's open is filed"
    );
    assert_eq!(
        editor.trust.get(),
        ProjectTrust::Trusted,
        "the premise: the trust of the project open is the new one's"
    );
    // The frame's `PreExtract` pump, before `before_agents` unmounts the
    // restricted project.
    shader_pump(&editor.engine.runtime);
    crate::project_scripts::follow_open_project(&editor.engine.runtime, &mut mounted, None);

    assert!(
        pipelines.set().is_empty(),
        "the restricted project's overlay is used while the trusted one opens: {:?}",
        pipelines.set()
    );
}

/// One run of the engine's shader pump over `runtime`.
fn shader_pump(runtime: &khora_sdk::Runtime) {
    let system = khora_sdk::inventory::iter::<khora_sdk::khora_data::ecs::DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == "shader_hot_reload")
        .expect("`shader_hot_reload` is registered");
    (system.run)(
        &mut khora_sdk::khora_data::ecs::World::new(),
        runtime,
        &mut khora_sdk::khora_core::lane::OutputDeck::new(),
        khora_sdk::khora_data::ecs::SystemContext {
            since: khora_sdk::khora_data::ecs::Tick(0),
            resumed: false,
            scope: None,
        },
    );
}

use khora_sdk::khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupId, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    BindGroupLayoutId, CommandBufferId,
};
use khora_sdk::khora_core::renderer::api::device::GraphicsAdapterInfo;
use khora_sdk::khora_core::renderer::api::pipeline::{
    ComputePipelineDescriptor, ComputePipelineId, ComputePipelineSpec, LayoutKey,
    PipelineLayoutDescriptor, PipelineLayoutId, PipelineSpec, RenderPipelineDescriptor,
    RenderPipelineId,
};
use khora_sdk::khora_core::renderer::api::resource::{
    BufferDescriptor, BufferId, SamplerDescriptor, SamplerId, TextureDescriptor, TextureFormat,
    TextureId, TextureViewDescriptor, TextureViewId,
};
use khora_sdk::khora_core::renderer::api::shader::{
    ShaderModuleDescriptor, ShaderModuleId, ShaderVariantKey,
};
use khora_sdk::khora_core::renderer::error::{RenderError, ResourceError};
use khora_sdk::khora_core::renderer::traits::{CommandEncoder, GraphicsDevice, PipelineSystem};

/// A pipeline system that records the overlays set and builds nothing.
#[derive(Default)]
struct OverlayLog(Mutex<Vec<String>>);

impl OverlayLog {
    /// The modules an overlay was set for, in order.
    fn set(&self) -> Vec<String> {
        self.0.lock().expect("the overlay log").clone()
    }
}

impl PipelineSystem for OverlayLog {
    fn layout(
        &self,
        _device: &dyn GraphicsDevice,
        _key: LayoutKey,
        _variant: &ShaderVariantKey,
    ) -> Result<BindGroupLayoutId, RenderError> {
        unreachable!()
    }
    fn inline_layout(
        &self,
        _device: &dyn GraphicsDevice,
        _label: &'static str,
        _entries: &[BindGroupLayoutEntry],
    ) -> Result<BindGroupLayoutId, RenderError> {
        unreachable!()
    }
    fn pipeline(
        &self,
        _device: &dyn GraphicsDevice,
        _spec: &PipelineSpec,
    ) -> Result<RenderPipelineId, RenderError> {
        unreachable!()
    }
    fn compute_pipeline(
        &self,
        _device: &dyn GraphicsDevice,
        _spec: &ComputePipelineSpec,
    ) -> Result<ComputePipelineId, RenderError> {
        unreachable!()
    }
    fn set_overlay_source(&self, logical_path: &str, _source: String) {
        self.0
            .lock()
            .expect("the overlay log")
            .push(logical_path.to_owned());
    }
    fn recompose_dirty(&self, _device: &dyn GraphicsDevice) -> Result<(), RenderError> {
        Ok(())
    }
}

/// A device the shader pump only hands on to `recompose_dirty`.
#[derive(Debug)]
struct NoDevice;

impl GraphicsDevice for NoDevice {
    fn create_shader_module(
        &self,
        _d: &ShaderModuleDescriptor,
    ) -> Result<ShaderModuleId, ResourceError> {
        unreachable!()
    }
    fn destroy_shader_module(&self, _id: ShaderModuleId) -> Result<(), ResourceError> {
        unreachable!()
    }
    fn create_render_pipeline(
        &self,
        _d: &RenderPipelineDescriptor,
    ) -> Result<RenderPipelineId, ResourceError> {
        unreachable!()
    }
    fn create_pipeline_layout(
        &self,
        _d: &PipelineLayoutDescriptor,
    ) -> Result<PipelineLayoutId, ResourceError> {
        unreachable!()
    }
    fn destroy_render_pipeline(&self, _id: RenderPipelineId) -> Result<(), ResourceError> {
        unreachable!()
    }
    fn create_compute_pipeline(
        &self,
        _d: &ComputePipelineDescriptor,
    ) -> Result<ComputePipelineId, ResourceError> {
        unreachable!()
    }
    fn destroy_compute_pipeline(&self, _id: ComputePipelineId) -> Result<(), ResourceError> {
        unreachable!()
    }
    fn create_bind_group_layout(
        &self,
        _d: &BindGroupLayoutDescriptor,
    ) -> Result<BindGroupLayoutId, ResourceError> {
        unreachable!()
    }
    fn create_bind_group(&self, _d: &BindGroupDescriptor) -> Result<BindGroupId, ResourceError> {
        unreachable!()
    }
    fn destroy_bind_group_layout(&self, _id: BindGroupLayoutId) -> Result<(), ResourceError> {
        unreachable!()
    }
    fn destroy_bind_group(&self, _id: BindGroupId) -> Result<(), ResourceError> {
        unreachable!()
    }
    fn create_buffer(&self, _d: &BufferDescriptor) -> Result<BufferId, ResourceError> {
        unreachable!()
    }
    fn create_buffer_with_data(
        &self,
        _d: &BufferDescriptor,
        _data: &[u8],
    ) -> Result<BufferId, ResourceError> {
        unreachable!()
    }
    fn destroy_buffer(&self, _id: BufferId) -> Result<(), ResourceError> {
        unreachable!()
    }
    fn write_buffer(&self, _id: BufferId, _offset: u64, _data: &[u8]) -> Result<(), ResourceError> {
        unreachable!()
    }
    fn write_buffer_async<'a>(
        &'a self,
        _id: BufferId,
        _offset: u64,
        _data: &'a [u8],
    ) -> Box<dyn std::future::Future<Output = Result<(), ResourceError>> + Send + 'static> {
        unreachable!()
    }
    fn create_texture(&self, _d: &TextureDescriptor) -> Result<TextureId, ResourceError> {
        unreachable!()
    }
    fn destroy_texture(&self, _id: TextureId) -> Result<(), ResourceError> {
        unreachable!()
    }
    fn write_texture(
        &self,
        _id: TextureId,
        _data: &[u8],
        _bytes_per_row: Option<u32>,
        _offset: khora_sdk::khora_core::math::dimension::Origin3D,
        _size: khora_sdk::khora_core::math::dimension::Extent3D,
    ) -> Result<(), ResourceError> {
        unreachable!()
    }
    fn create_texture_view(
        &self,
        _id: TextureId,
        _d: &TextureViewDescriptor,
    ) -> Result<TextureViewId, ResourceError> {
        unreachable!()
    }
    fn destroy_texture_view(&self, _id: TextureViewId) -> Result<(), ResourceError> {
        unreachable!()
    }
    fn create_sampler(&self, _d: &SamplerDescriptor) -> Result<SamplerId, ResourceError> {
        unreachable!()
    }
    fn destroy_sampler(&self, _id: SamplerId) -> Result<(), ResourceError> {
        unreachable!()
    }
    fn create_command_encoder(&self, _label: Option<&str>) -> Box<dyn CommandEncoder> {
        unreachable!()
    }
    fn submit_command_buffer(&self, _command_buffer: CommandBufferId) {
        unreachable!()
    }
    fn get_surface_format(&self) -> Option<TextureFormat> {
        unreachable!()
    }
    fn get_surface_size(&self) -> (u32, u32) {
        unreachable!()
    }
    fn get_adapter_info(&self) -> GraphicsAdapterInfo {
        unreachable!()
    }
    fn supports_feature(&self, _feature_name: &str) -> bool {
        unreachable!()
    }
}
