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

//! A glTF scene as a prefab record: one entity per node under a root, each
//! showing its primitives' mesh and material sub-assets.
//!
//! Placed in a level, the record is a prefab instance: the level keeps the
//! link and the designer's changes, and every load expands the file as it is
//! now. A change is kept against the member it was made on, so each member's
//! identity must survive a re-export. It comes from the node's **path of
//! names** from the scene's roots — `Body[0]/Wheel[1]` — not from its index:
//! an exporter reorders nodes freely, while renaming one is the author's act,
//! and a change whose node was renamed is reported orphaned by the load
//! instead of landing on another node.
//!
//! A path segment is the node's name, escaped, with its ordinal among the
//! earlier siblings of the same name (`[0]` for the first, so a second
//! `Wheel` does not change the first one's key); an unnamed node is `#i`, its
//! place among its siblings. A primitive past the first is a child entity
//! whose segment is `#prim{p}` — a `#` no escaped name can start with.
//!
//! The key of a member is composed from its parent's key and its own
//! segment ([`member_key`]), so it is a function of the whole path while each
//! node costs only its own segment: a deep hierarchy costs its size, not its
//! size times its depth.

use std::collections::HashMap;

use anyhow::{anyhow, bail};
use khora_core::asset::AssetUUID;
use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;
use khora_core::math::{Mat4, Quaternion, Vec3, Vec4};
use khora_data::ecs::{GlobalTransform, MaterialRef, MeshRef, Name, Transform, World};
use khora_data::scene::{capture_subtree, SceneRecord};

/// What every key of a glTF member starts with, apart from other named ids.
const KEY_PREFIX: &str = "gltf-node:";

/// How far from perpendicular two axes of a node's `matrix` may be before
/// the matrix counts as a shear, which an entity's translation, rotation and
/// scale cannot hold (glTF requires a decomposable matrix). Measured on the
/// unit axes, so it holds the same at any scale.
const ORTHOGONALITY_TOLERANCE: f32 = 1e-4;

/// The key of the scene's root.
pub(super) fn root_key() -> PersistentId {
    PersistentId::named(KEY_PREFIX)
}

/// The key of the member whose path segment is `segment`, under the member
/// keyed `parent`.
pub(super) fn member_key(parent: PersistentId, segment: &str) -> PersistentId {
    PersistentId::named(&format!("{KEY_PREFIX}{:016x}/{segment}", parent.to_bits()))
}

/// The record of scene `scene` of `document` (`None`: the default scene,
/// else the first, else every mesh once), its sub-assets named under
/// `container`; the root is named after the scene, else `file_stem`.
pub(super) fn scene_record(
    document: &gltf::Document,
    scene: Option<usize>,
    container: AssetUUID,
    file_stem: &str,
) -> anyhow::Result<SceneRecord> {
    let scene = match scene {
        Some(index) => Some(
            document
                .scenes()
                .nth(index)
                .ok_or_else(|| anyhow!("the glTF has no scene{index}"))?,
        ),
        None => document
            .default_scene()
            .or_else(|| document.scenes().next()),
    };
    let root_name = scene
        .as_ref()
        .and_then(|scene| scene.name())
        .unwrap_or(file_stem);

    let mut builder = Builder {
        world: World::new(),
        container,
        edges: Vec::new(),
    };
    let root = builder.spawn(Name::new(root_name), Transform::identity(), root_key())?;
    match scene {
        Some(scene) => {
            builder.place_nodes(scene.nodes().collect(), root, document.nodes().len())?
        }
        None => builder.place_meshes(document, root)?,
    }
    builder.attach()?;
    capture_subtree(&builder.world, root).map_err(|e| anyhow!("capturing the glTF scene: {e:?}"))
}

/// The scratch world a record is captured from.
struct Builder {
    world: World,
    container: AssetUUID,
    /// Each member and its parent, with the member's depth, in file order.
    edges: Vec<(EntityId, EntityId, usize)>,
}

/// A node waiting to be placed: under `parent` (keyed `parent_key`), at
/// `depth`, as path segment `segment`.
struct Pending<'a> {
    node: gltf::Node<'a>,
    parent: EntityId,
    parent_key: PersistentId,
    segment: String,
    depth: usize,
}

impl Builder {
    /// Places `roots` and every node under them below `root`, depth first in
    /// file order.
    fn place_nodes(
        &mut self,
        roots: Vec<gltf::Node<'_>>,
        root: EntityId,
        node_count: usize,
    ) -> anyhow::Result<()> {
        let mut reached = vec![false; node_count];
        let mut pending: Vec<Pending<'_>> = Vec::new();
        push_siblings(&mut pending, roots, root, root_key(), 1);
        while let Some(Pending {
            node,
            parent,
            parent_key,
            segment,
            depth,
        }) = pending.pop()
        {
            let seen = reached
                .get_mut(node.index())
                .ok_or_else(|| anyhow!("glTF node {} does not exist", node.index()))?;
            if std::mem::replace(seen, true) {
                bail!(
                    "glTF node {} is reached twice: the node hierarchy is not a tree",
                    node.index()
                );
            }
            let name = node
                .name()
                .map_or_else(|| format!("Node{}", node.index()), str::to_owned);
            let key = member_key(parent_key, &segment);
            let entity = self.spawn(Name::new(name), node_transform(&node)?, key)?;
            self.edges.push((entity, parent, depth));
            if let Some(mesh) = node.mesh() {
                self.show_mesh(&mesh, entity, key, depth)?;
            }
            push_siblings(
                &mut pending,
                node.children().collect(),
                entity,
                key,
                depth + 1,
            );
        }
        Ok(())
    }

    /// Places every mesh once below `root`, for a file without scenes.
    fn place_meshes(&mut self, document: &gltf::Document, root: EntityId) -> anyhow::Result<()> {
        for mesh in document.meshes() {
            let name = mesh
                .name()
                .map_or_else(|| format!("Mesh{}", mesh.index()), str::to_owned);
            let key = member_key(root_key(), &format!("#mesh{}", mesh.index()));
            let entity = self.spawn(Name::new(name), Transform::identity(), key)?;
            self.edges.push((entity, root, 1));
            self.show_mesh(&mesh, entity, key, 1)?;
        }
        Ok(())
    }

    /// Shows `mesh` on `entity` (keyed `key`, at `depth`): its first
    /// primitive on the entity itself, each further one on a child of its own.
    fn show_mesh(
        &mut self,
        mesh: &gltf::Mesh<'_>,
        entity: EntityId,
        key: PersistentId,
        depth: usize,
    ) -> anyhow::Result<()> {
        for primitive in mesh.primitives() {
            let shown_on = if primitive.index() == 0 {
                entity
            } else {
                let segment = format!("#prim{}", primitive.index());
                let child = self.spawn(
                    Name::new(&segment[1..]),
                    Transform::identity(),
                    member_key(key, &segment),
                )?;
                self.edges.push((child, entity, depth + 1));
                child
            };
            let mesh_label = format!("mesh{}/prim{}", mesh.index(), primitive.index());
            let material_label = primitive
                .material()
                .index()
                .map_or_else(|| "material_default".to_owned(), |k| format!("material{k}"));
            let mesh_ref = MeshRef::Asset(AssetUUID::sub_asset(self.container, &mesh_label));
            let material_ref =
                MaterialRef::Asset(AssetUUID::sub_asset(self.container, &material_label));
            self.world
                .add_component(shown_on, mesh_ref)
                .map_err(|e| anyhow!("showing '{mesh_label}': {e:?}"))?;
            self.world
                .add_component(shown_on, material_ref)
                .map_err(|e| anyhow!("showing '{material_label}': {e:?}"))?;
        }
        Ok(())
    }

    /// An entity named `name`, placed by `transform`, keyed `key`.
    fn spawn(
        &mut self,
        name: Name,
        transform: Transform,
        key: PersistentId,
    ) -> anyhow::Result<EntityId> {
        let entity = self
            .world
            .try_spawn((name, transform, GlobalTransform::default()))
            .map_err(|e| anyhow!("spawning a glTF member: {e:?}"))?;
        self.world.set_persistent_id(entity, key);
        Ok(entity)
    }

    /// Links every member to its parent, deepest first so a parent is not yet
    /// linked upwards when its children join it: `set_parent`'s cycle guard
    /// then stops at once instead of walking the whole chain above. The sort
    /// is stable, so each parent still lists its children in file order.
    fn attach(&mut self) -> anyhow::Result<()> {
        let mut edges = std::mem::take(&mut self.edges);
        edges.sort_by_key(|&(_, _, depth)| std::cmp::Reverse(depth));
        for (child, parent, _) in edges {
            if !self.world.set_parent(child, Some(parent)) {
                bail!("the glTF hierarchy could not be built: a member refused its parent");
            }
        }
        Ok(())
    }
}

/// Queues `siblings` (children of `parent`, keyed `parent_key`) so they pop
/// in file order, each with its own path segment.
fn push_siblings<'a>(
    pending: &mut Vec<Pending<'a>>,
    siblings: Vec<gltf::Node<'a>>,
    parent: EntityId,
    parent_key: PersistentId,
    depth: usize,
) {
    let mut seen_names: HashMap<&str, usize> = HashMap::new();
    let mut queued: Vec<Pending<'a>> = Vec::with_capacity(siblings.len());
    for (place, node) in siblings.iter().enumerate() {
        let segment = match node.name() {
            Some(name) => {
                let ordinal = seen_names.entry(name).or_insert(0);
                let segment = format!("{}[{ordinal}]", escape(name));
                *ordinal += 1;
                segment
            }
            None => format!("#{place}"),
        };
        queued.push(Pending {
            node: node.clone(),
            parent,
            parent_key,
            segment,
            depth,
        });
    }
    pending.extend(queued.into_iter().rev());
}

/// `name` with the characters a path gives a meaning to escaped, so two
/// different trees never spell the same path.
fn escape(name: &str) -> String {
    let mut escaped = String::with_capacity(name.len());
    for c in name.chars() {
        if matches!(c, '\\' | '/' | '[' | ']' | '#') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

/// The node's local transform as translation, rotation and scale. A value
/// that is not a finite number, or a `matrix` that is no translation,
/// rotation and scale (a shear, a projection), is refused.
fn node_transform(node: &gltf::Node<'_>) -> anyhow::Result<Transform> {
    let index = node.index();
    match node.transform() {
        gltf::scene::Transform::Decomposed {
            translation,
            rotation,
            scale,
        } => {
            if !translation
                .iter()
                .chain(&rotation)
                .chain(&scale)
                .all(|v| v.is_finite())
            {
                bail!("glTF node {index}'s transform holds a value that is not a finite number");
            }
            let [x, y, z, w] = rotation;
            Ok(Transform::new(
                Vec3::new(translation[0], translation[1], translation[2]),
                Quaternion::new(x, y, z, w),
                Vec3::new(scale[0], scale[1], scale[2]),
            ))
        }
        gltf::scene::Transform::Matrix { matrix } => decompose(index, &matrix),
    }
}

/// `matrix` (column-major) as translation, rotation and scale.
///
/// Each axis column is a unit direction times a scale; the directions must be
/// perpendicular, or the matrix shears. An axis scaled to zero has no
/// direction of its own: it is completed from the others, so a flattened
/// node keeps the rotation it shows. A mirror (negative determinant) is a
/// negative scale on the first axis.
fn decompose(index: usize, matrix: &[[f32; 4]; 4]) -> anyhow::Result<Transform> {
    if !matrix.iter().flatten().all(|v| v.is_finite()) {
        bail!("glTF node {index}'s matrix holds a value that is not a finite number");
    }
    let projective = [matrix[0][3], matrix[1][3], matrix[2][3], matrix[3][3] - 1.0];
    if projective.iter().any(|v| v.abs() > ORTHOGONALITY_TOLERANCE) {
        bail!("glTF node {index}'s matrix is a projection, not a translation, rotation and scale");
    }

    let columns = [0, 1, 2].map(|i| Vec3::new(matrix[i][0], matrix[i][1], matrix[i][2]));
    let mut scale = columns.map(|column| column.length());
    let directions: [Option<Vec3>; 3] = [0, 1, 2].map(|i| {
        (scale[i] > f32::MIN_POSITIVE && scale[i].is_finite())
            .then(|| columns[i] * (1.0 / scale[i]))
    });
    for (a, b) in [(0, 1), (0, 2), (1, 2)] {
        if let (Some(u), Some(v)) = (directions[a], directions[b]) {
            if u.dot(v).abs() > ORTHOGONALITY_TOLERANCE {
                bail!(
                    "glTF node {index}'s matrix shears: it is not a translation, rotation and scale"
                );
            }
        }
    }

    let mut axes = complete_basis(directions);
    if axes[0].dot(axes[1].cross(axes[2])) < 0.0 {
        axes[0] = axes[0] * -1.0;
        scale[0] = -scale[0];
    }
    let rotation = Quaternion::from_rotation_matrix(&Mat4::from_cols(
        Vec4::new(axes[0].x, axes[0].y, axes[0].z, 0.0),
        Vec4::new(axes[1].x, axes[1].y, axes[1].z, 0.0),
        Vec4::new(axes[2].x, axes[2].y, axes[2].z, 0.0),
        Vec4::new(0.0, 0.0, 0.0, 1.0),
    ))
    .normalize();
    Ok(Transform::new(
        Vec3::new(matrix[3][0], matrix[3][1], matrix[3][2]),
        rotation,
        Vec3::new(scale[0], scale[1], scale[2]),
    ))
}

/// Three perpendicular unit axes, keeping the known `directions` and
/// completing the missing ones (axes scaled to zero) right-handed.
fn complete_basis(directions: [Option<Vec3>; 3]) -> [Vec3; 3] {
    let known: Vec<usize> = (0..3).filter(|&i| directions[i].is_some()).collect();
    match known.as_slice() {
        [_, _, _] => directions.map(|d| d.unwrap_or(Vec3::X)),
        [a, b] => {
            let missing = 3 - a - b;
            let mut axes = directions.map(|d| d.unwrap_or(Vec3::X));
            axes[missing] = axes[(missing + 1) % 3].cross(axes[(missing + 2) % 3]);
            axes
        }
        [a] => {
            let known_axis = directions[*a].unwrap_or(Vec3::X);
            let helper = if known_axis.x.abs() < 0.9 {
                Vec3::X
            } else {
                Vec3::Y
            };
            let next = known_axis.cross(helper).normalize();
            let last = known_axis.cross(next);
            let mut axes = [Vec3::X; 3];
            axes[*a] = known_axis;
            axes[(*a + 1) % 3] = next;
            axes[(*a + 2) % 3] = last;
            axes
        }
        _ => [Vec3::X, Vec3::Y, Vec3::Z],
    }
}
