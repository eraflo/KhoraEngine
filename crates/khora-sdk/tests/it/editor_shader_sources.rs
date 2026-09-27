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

//! The two shaders handed over as raw WGSL strings — text and the egui
//! overlay — reach their consumers through the SDK, and each is the shader its
//! consumer expects: it parses, validates, exposes `vs_main` / `fs_main`, and
//! declares the bindings that consumer's pipeline layout binds.
//!
//! Read through the paths the editor and the sandbox use, so moving the
//! `.wgsl` files or the constants shows up here rather than at editor boot.

use khora_sdk::EGUI_WGSL;
use khora_sdk::TEXT_WGSL;

/// An entry point as `(name, stage)`.
type EntryPoint = (String, naga::ShaderStage);
/// A resource binding as `(name, group, binding)`.
type Binding = (String, u32, u32);

/// Parses and validates `source`, then returns its entry points as
/// `(name, stage)` and its resource bindings as `(name, group, binding)`,
/// both sorted.
fn shape(label: &str, source: &str) -> (Vec<EntryPoint>, Vec<Binding>) {
    let module = naga::front::wgsl::parse_str(source)
        .unwrap_or_else(|e| panic!("{label} does not parse: {e}"));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .unwrap_or_else(|e| panic!("{label} does not validate: {e:?}"));

    let mut entry_points: Vec<EntryPoint> = module
        .entry_points
        .iter()
        .map(|ep| (ep.name.clone(), ep.stage))
        .collect();
    entry_points.sort_by(|a, b| a.0.cmp(&b.0));

    let mut bindings: Vec<Binding> = module
        .global_variables
        .iter()
        .filter_map(|(_, var)| {
            let binding = var.binding.as_ref()?;
            Some((
                var.name.clone().unwrap_or_default(),
                binding.group,
                binding.binding,
            ))
        })
        .collect();
    bindings.sort_by_key(|b| (b.1, b.2));

    (entry_points, bindings)
}

fn owned(entries: &[(&str, u32, u32)]) -> Vec<Binding> {
    entries
        .iter()
        .map(|(name, group, binding)| ((*name).to_owned(), *group, *binding))
        .collect()
}

fn vs_fs() -> Vec<EntryPoint> {
    vec![
        ("fs_main".to_owned(), naga::ShaderStage::Fragment),
        ("vs_main".to_owned(), naga::ShaderStage::Vertex),
    ]
}

#[test]
fn text_wgsl_is_the_text_shader_and_validates() {
    assert!(!TEXT_WGSL.trim().is_empty(), "TEXT_WGSL is empty");
    let (entry_points, bindings) = shape("TEXT_WGSL", TEXT_WGSL);
    assert_eq!(entry_points, vs_fs());
    assert_eq!(
        bindings,
        owned(&[("globals", 0, 0), ("t_diffuse", 1, 0), ("s_diffuse", 1, 1)])
    );
}

#[test]
fn egui_wgsl_is_the_egui_shader_and_validates() {
    assert!(!EGUI_WGSL.trim().is_empty(), "EGUI_WGSL is empty");
    let (entry_points, bindings) = shape("EGUI_WGSL", EGUI_WGSL);
    assert_eq!(entry_points, vs_fs());
    assert_eq!(
        bindings,
        owned(&[("screen_size", 0, 0), ("t_egui", 1, 0), ("s_egui", 1, 1)])
    );
}
