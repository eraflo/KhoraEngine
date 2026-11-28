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

//! Bytes lying about their type, refused.

use super::*;

/// **A file whose bytes lie about its type is refused.**
///
/// Through a guarded file loader, a `.png` holding text fails to load, is
/// refused as the wrong type (`texture`), quarantined and reported through the
/// service's refusals; a `.png` holding a PNG loads.
#[test]
fn a_file_whose_bytes_lie_about_its_type_is_refused() {
    use crate::asset::{AssetService, FileLoader, IndexBuilder, TextureDecoder};
    use khora_core::asset::AssetUUID;
    use khora_core::renderer::api::resource::CpuTexture;

    let dir = TempDir::new().expect("a temp dir");
    let root = dir.path().join("assets");
    fs::create_dir_all(root.join("textures")).expect("the folder is made");
    fs::write(
        root.join("textures").join("liar.png"),
        b"this is a note, not an image\n",
    )
    .expect("writes");
    image::RgbaImage::new(2, 2)
        .save(root.join("textures").join("honest.png"))
        .expect("a PNG is written");

    let problems = Arc::new(Problems::default());
    let refusals = reported_to(&problems);
    let index = IndexBuilder::new(crate::trust::FileGuard::new(
        &root,
        crate::trust::Trust::Project,
        crate::trust::Refusals::detached(),
    ))
    .build_index_bytes()
    .expect("the folder indexes");
    let loader = FileLoader::guarded(FileGuard::new(&root, Trust::Project, Arc::clone(&refusals)));
    let mut service = AssetService::new(
        &index,
        Box::new(loader),
        Arc::new(khora_telemetry::MetricsRegistry::new()),
        None,
    )
    .expect("a service over the index");
    service
        .register_decoder::<CpuTexture>(khora_core::asset::kinds::TEXTURE.name(), TextureDecoder);
    service.set_refusals(Arc::clone(&refusals));

    service
        .load::<CpuTexture>(&AssetUUID::new_v5("textures/honest.png"))
        .expect("a PNG holding a PNG loads");
    assert!(service
        .load::<CpuTexture>(&AssetUUID::new_v5("textures/liar.png"))
        .is_err());

    let wrong_type = refusals.quarantined().into_iter().find(|refusal| {
        refusal.path.ends_with("liar.png")
            && matches!(&refusal.reason, SecurityReason::WrongType { .. })
    });
    let wrong_type = wrong_type.expect("the liar is refused as the wrong type, and quarantined");
    assert_eq!(
        wrong_type.reason,
        SecurityReason::WrongType {
            expected: "texture".to_owned()
        }
    );
    assert!(
        security(&problems).iter().any(|problem| {
            problem.severity == ProblemSeverity::Error
                && problem
                    .module
                    .as_deref()
                    .is_some_and(|module| module.ends_with("liar.png"))
        }),
        "the refusal is reported: {:?}",
        security(&problems)
    );
    assert!(
        !refusals
            .quarantined()
            .iter()
            .any(|refusal| refusal.path.ends_with("honest.png")),
        "the honest file is not refused"
    );
}

/// The decoder registry answers for the decoder of a type, and for none.
#[test]
fn the_decoder_registry_sniffs_with_the_decoder_of_a_type() {
    use crate::asset::{DecoderRegistry, TextureDecoder};
    use khora_core::renderer::api::resource::CpuTexture;

    let mut registry = DecoderRegistry::new(Arc::new(khora_telemetry::MetricsRegistry::new()));
    registry.register::<CpuTexture>("texture", TextureDecoder);

    assert_eq!(registry.sniff("texture", sniffs::PNG), Some(true));
    assert_eq!(registry.sniff("texture", sniffs::TEXT), Some(false));
    assert_eq!(registry.sniff("nothing", sniffs::PNG), None);
}
