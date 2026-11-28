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

//! The steady state: an unchanged project does no work.

use super::*;

/// Once a change is applied, the following ticks find nothing to do.
///
/// The change is not applied again and the copy stored since is not
/// replaced, so the GPU copy made from it stays.
#[test]
fn an_unchanged_project_does_no_work() {
    let kmat_path = "materials/plain.kmat";
    let mat = AssetUUID::new_v5(kmat_path);
    let project = Project::new(&[(kmat_path, kmat(0.5, None))]);
    let mut world = World::new();
    let entity = world.spawn(AssetRef::<Box<dyn Material>>::Asset(mat));
    project.tick(&mut world);
    let resolved = project.load_count();

    project.write(kmat_path, &kmat(0.7, None));
    project.invalidate(&mat);
    project.tick(&mut world);
    assert_eq!(
        project.load_count(),
        resolved + 1,
        "the change is applied: the material is decoded again"
    );
    assert_eq!(material_uuid(&project, &world, entity), mat);
    project.watch::<Box<dyn Material>>();
    let applied = project.load_count();

    project.tick(&mut world);
    project.tick(&mut world);

    assert_eq!(project.load_count(), applied, "no change, no decode");
    assert!(
        project.arrived::<Box<dyn Material>>(&mat).is_empty(),
        "the copy stored after the change stays"
    );
}
