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

//! A save read for merging: each of its records as this build reads it, and
//! what the save says of each entity.

use super::*;

/// `record` as this build reads it: loaded into a world of its own, among
/// `entities`, and written down again — components by today's names, fields
/// by today's names, every value at today's type.
///
/// `entities` are every entity the values may refer to, so a reference to
/// one the record has no row for is still a reference, not an outside one.
///
/// A component declared while the engine ran is read under `under` where it
/// names it — the declaration the record is compared against — and under the
/// record's own otherwise.
///
/// With it, what the reading adapted: a value read at today's type is no
/// longer the one the file holds.
pub(super) fn as_read(
    record: &SceneRecord,
    entities: Vec<PersistentId>,
    kept: Kept,
    under: &[DeclaredRecord],
) -> Result<(SceneRecord, LoadReport), LoadFailure> {
    let mut world = World::new();
    register_all(&mut world, under);
    let whole = SceneRecord {
        entities,
        instances: Vec::new(),
        pages: record.pages.clone(),
        declared: record.declared.clone(),
    };
    let report = prepare_kept(&mut world, &whole, kept)?
        .commit(&mut world, Identity::Keep)
        .report;
    let captured = match kept {
        Kept::Scene => capture_world(&world),
        Kept::Game => capture_game(&world),
    };
    captured
        .map(|record| (record, report))
        .map_err(|error| LoadFailure {
            message: error.to_string(),
            report: Default::default(),
        })
}

/// `scene` as this build reads it, its declared components under `under`.
pub(crate) fn scene_as_read(
    scene: &SceneRecord,
    under: &[DeclaredRecord],
) -> Result<(SceneRecord, LoadReport), LoadFailure> {
    as_read(scene, scene.entities.clone(), Kept::Scene, under)
}

/// A record that could not be read, and why.
pub(super) type Unreadable = Box<(SceneRecord, LoadFailure)>;

/// A save, read for merging: its values as this build reads them, and what it
/// says of each entity.
pub(super) struct Read {
    pub(super) base: SceneRecord,
    pub(super) changes: SceneRecord,
    pub(super) before: SceneRecord,
    pub(super) destroyed: HashSet<PersistentId>,
    pub(super) created: HashSet<PersistentId>,
    pub(super) listed: HashSet<PersistentId>,
    pub(super) removed: HashMap<PersistentId, Vec<String>>,
    /// What reading the scene, the save's changes and its before-image
    /// adapted, for the load to report: the merge sees only the values as
    /// read.
    pub(super) report: Vec<ReportEntry>,
}

impl Read {
    /// Every declaration the records read describe — the scene's last, so it
    /// wins: the save was read under it.
    pub(super) fn declarations(&self) -> impl Iterator<Item = &DeclaredRecord> {
        self.before
            .declared
            .iter()
            .chain(&self.changes.declared)
            .chain(&self.base.declared)
    }

    /// Reads `save` against `base`. What cannot be read is the error, with
    /// the record that could not.
    pub(super) fn new(base: &SceneRecord, save: &SaveRecord) -> Result<Read, Unreadable> {
        let (base, base_report) =
            scene_as_read(base, &[]).map_err(|failure| Box::new((base.clone(), failure)))?;
        let in_base: HashSet<PersistentId> = base.entities.iter().copied().collect();
        let mut everyone = base.entities.clone();
        let mut seen = in_base.clone();
        for id in save
            .order
            .iter()
            .chain(&save.changes.entities)
            .chain(&save.before.entities)
            .chain(&referenced(&save.changes))
            .chain(&referenced(&save.before))
        {
            if seen.insert(*id) {
                everyone.push(*id);
            }
        }
        let unreadable = |record: &SceneRecord, failure: LoadFailure| {
            let record = SceneRecord {
                entities: everyone.clone(),
                instances: Vec::new(),
                pages: record.pages.clone(),
                declared: record.declared.clone(),
            };
            Box::new((record, failure))
        };
        // The save read as the scene, as it is now, declares its components:
        // the three-way merge compares values of one declaration.
        let (changes, changes_report) =
            as_read(&save.changes, everyone.clone(), Kept::Game, &base.declared)
                .map_err(|failure| unreadable(&save.changes, failure))?;
        let (before, before_report) =
            as_read(&save.before, everyone.clone(), Kept::Scene, &base.declared)
                .map_err(|failure| unreadable(&save.before, failure))?;
        // A reference to an entity no record holds is read as it was written,
        // and the load reports it where it lands; everything else the reading
        // adapted is gone from the values the merge sees.
        let report: Vec<ReportEntry> = [base_report, changes_report, before_report]
            .into_iter()
            .flat_map(|read| read.entries)
            .filter(|entry| entry.kind != ReportKind::DeadReference)
            .collect();
        // A name the save wrote is read as today's: a component renamed since
        // is the same component.
        let today = |name: &String| {
            registration_named(name).map_or_else(|| name.clone(), |reg| reg.type_name.to_owned())
        };
        Ok(Read {
            destroyed: save.destroyed.iter().copied().collect(),
            // An entity the scene has is the scene's, whatever a save says.
            created: save
                .created
                .iter()
                .filter(|id| !in_base.contains(id))
                .copied()
                .collect(),
            listed: save.changes.entities.iter().copied().collect(),
            removed: save
                .removed
                .iter()
                .map(|entry| (entry.entity, entry.components.iter().map(today).collect()))
                .collect(),
            base,
            changes,
            before,
            report,
        })
    }

    /// Whether the save says anything of `id`, one of the scene's entities.
    pub(super) fn diverged(&self, id: PersistentId) -> bool {
        self.listed.contains(&id) || self.removed.contains_key(&id)
    }

    /// The components the game removed from `id`, by today's names.
    pub(super) fn removed_from(&self, id: PersistentId) -> &[String] {
        self.removed.get(&id).map_or(&[][..], Vec::as_slice)
    }

    /// The order the merged world lists its entities in: the save's, less
    /// what the scene lost, with what the scene gained since right after the
    /// entity before it in the scene. Reports each entity the save changed
    /// that the scene no longer has.
    pub(super) fn order(
        &self,
        save: &SaveRecord,
        report: &mut Vec<ReportEntry>,
    ) -> Vec<PersistentId> {
        let in_base: HashSet<PersistentId> = self.base.entities.iter().copied().collect();
        let mut seen = HashSet::new();
        let mut order = Vec::with_capacity(save.order.len());
        for id in save.order.iter().chain(&save.changes.entities) {
            if self.destroyed.contains(id) || !seen.insert(*id) {
                continue;
            }
            if self.created.contains(id) || in_base.contains(id) {
                order.push(*id);
            } else if self.listed.contains(id) {
                report.push(ReportEntry {
                    entity: Some(*id),
                    component: None,
                    path: String::new(),
                    kind: ReportKind::RemovedFromScene,
                });
            }
        }
        let mut after: HashMap<Option<PersistentId>, Vec<PersistentId>> = HashMap::new();
        let mut previous = None;
        for id in &self.base.entities {
            if seen.contains(id) {
                previous = Some(*id);
            } else if !self.destroyed.contains(id) {
                after.entry(previous).or_default().push(*id);
                previous = Some(*id);
            }
        }
        let mut entities = Vec::with_capacity(order.len());
        follow(None, &mut after, &mut entities);
        for id in order {
            entities.push(id);
            follow(Some(id), &mut after, &mut entities);
        }
        entities
    }
}

/// Lays out the entities the author added to the scene after `key`, each
/// followed by the ones added after it.
pub(super) fn follow(
    key: Option<PersistentId>,
    after: &mut HashMap<Option<PersistentId>, Vec<PersistentId>>,
    out: &mut Vec<PersistentId>,
) {
    let mut stack: Vec<PersistentId> = after.remove(&key).unwrap_or_default();
    stack.reverse();
    while let Some(added) = stack.pop() {
        out.push(added);
        let mut more = after.remove(&Some(added)).unwrap_or_default();
        more.reverse();
        stack.extend(more);
    }
}
