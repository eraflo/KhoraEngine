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

//! What a prefab spawn costs, and what its fingerprint names.

use khora_script::vm::instruction::Instruction;

use super::*;

/// The fingerprint of function `name` of `main`, compiled.
fn fingerprint(main: &str, name: &str) -> u64 {
    build(main)
        .function(name)
        .unwrap_or_else(|| panic!("`{name}` is a function of the program"))
        .fingerprint
}

/// **A prefab spawn costs one and the fields its patches name**, as a plain
/// spawn does.
#[test]
fn a_prefab_spawn_costs_one_and_its_fields() {
    let spawn = |count| Instruction::SpawnPrefab {
        dst: 0,
        prefab: 1,
        position: 2,
        facing: 3,
        spawn: 0,
        base: 4,
        count,
    };
    let plain = |count| Instruction::SpawnEntity {
        dst: 0,
        position: 2,
        spawn: 0,
        base: 4,
        count,
    };

    for count in [0, 1, 3, 12] {
        assert_eq!(spawn(count).cost(), 1 + u64::from(count), "{count} fields");
        assert_eq!(spawn(count).cost(), plain(count).cost());
    }
}

/// **A prefab spawn is other code than a plain spawn** of the same
/// components: a save suspended in one never resumes into the other.
#[test]
fn a_prefab_spawn_is_other_code_than_a_plain_spawn() {
    let prefab = fingerprint(
        "fn void Fire(Prefab bullet) {
             Spawn(bullet, Health { current: 3 }, at: Vec3(0.0, 1.0, 0.0));
         }",
        "Fire",
    );
    let plain = fingerprint(
        "fn void Fire(Prefab bullet) {
             Spawn(Vec3(0.0, 1.0, 0.0), Health { current: 3 });
         }",
        "Fire",
    );

    assert_ne!(prefab, plain);
}

/// **The fingerprint names the components a prefab spawn writes**: two
/// spawns writing another component, or another field, are other code.
#[test]
fn a_prefab_spawns_fingerprint_names_what_it_writes() {
    let health = fingerprint(
        "fn void Fire(Prefab bullet) {
             Spawn(bullet, Health { current: 3 }, at: Vec3(0.0, 1.0, 0.0));
         }",
        "Fire",
    );
    let max = fingerprint(
        "fn void Fire(Prefab bullet) {
             Spawn(bullet, Health { max: 3 }, at: Vec3(0.0, 1.0, 0.0));
         }",
        "Fire",
    );
    let collider = fingerprint(
        "fn void Fire(Prefab bullet) {
             Spawn(bullet, Collider { friction: 3.0 }, at: Vec3(0.0, 1.0, 0.0));
         }",
        "Fire",
    );

    assert_ne!(health, max, "another field");
    assert_ne!(health, collider, "another component");
    assert_eq!(
        health,
        fingerprint(
            "fn void Fire(Prefab bullet) {
                 Spawn(bullet, Health { current: 3 }, at: Vec3(0.0, 1.0, 0.0));
             }",
            "Fire",
        ),
        "the same code fingerprints the same"
    );
}

/// **Renaming the material's local is no change of code**: locals are
/// registers, so `e.SetMaterial(m)` and `e.SetMaterial(paint)` fingerprint
/// the same, and a turn suspended in one resumes exactly in the other.
#[test]
fn renaming_the_material_local_keeps_the_fingerprint() {
    let m = fingerprint(
        "fn void Paint(Entity e, Material? m) { e.SetMaterial(m); }",
        "Paint",
    );
    let paint = fingerprint(
        "fn void Paint(Entity e, Material? paint) { e.SetMaterial(paint); }",
        "Paint",
    );

    assert_eq!(m, paint);
}

/// Renaming the prefab's local is no change of code either.
#[test]
fn renaming_the_prefab_local_keeps_the_fingerprint() {
    let bullet = fingerprint(
        "fn void Fire(Prefab bullet) { Spawn(bullet, at: Vec3(0.0, 0.0, 0.0)); }",
        "Fire",
    );
    let shell = fingerprint(
        "fn void Fire(Prefab shell) { Spawn(shell, at: Vec3(0.0, 0.0, 0.0)); }",
        "Fire",
    );

    assert_eq!(bullet, shell);
}
