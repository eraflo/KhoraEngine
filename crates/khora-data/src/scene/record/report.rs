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

//! What reading a save changed, said out loud.
//!
//! A save older than its code is the normal state of a project, and reading
//! one adapts it: a field defaults, another is dropped, a third was renamed.
//! None of that is an error, and none of it may be silent either — an author
//! who renamed a field and lost its value has to be told. So every read that
//! adapts something says what, where.
//!
//! The reader notes what it does as it goes (see `watch`); the report names
//! those notes by the value the read produced, written again — the names the
//! code uses today.

use std::collections::HashMap;

use serde::de::DeserializeOwned;
use serde::Serialize;

use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;

use super::de::read_watched;
use super::watch::{Event, Node, Step, Tree};
use super::{
    to_record, EntityRef, Record, RecordError, ReferenceReader, ReferenceWriter, VariantPayload,
};

/// Everything a load adapted.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LoadReport {
    /// One entry per adaptation, in the order they were found.
    pub entries: Vec<ReportEntry>,
}

impl LoadReport {
    /// Whether the load read everything exactly as written.
    pub fn is_clean(&self) -> bool {
        self.entries.is_empty()
    }
}

/// One thing a load adapted.
#[derive(Debug, Clone, PartialEq)]
pub struct ReportEntry {
    /// The entity it happened on, when there is one.
    pub entity: Option<PersistentId>,
    /// The component it happened in, by name, when there is one.
    pub component: Option<String>,
    /// Where inside the value: `translation.x`, `lights[2]`, `Perspective.fov`.
    pub path: String,
    /// What happened there.
    pub kind: ReportKind,
}

/// What a load did to one place in a value.
#[derive(Debug, Clone, PartialEq)]
pub enum ReportKind {
    /// The save predates this field; it took its default.
    Defaulted,
    /// The code no longer has this field; its value was dropped.
    Dropped,
    /// The save wrote this field, or variant, under an older name.
    Renamed {
        /// The name the save used.
        from: String,
    },
    /// A number was read at a wider type than it was written.
    Widened,
    /// The save holds a component type declared retired; it was skipped.
    Retired,
    /// A reference named an entity the save does not hold.
    DeadReference,
}

/// Reads a `T` from `record` and says what the read adapted.
///
/// The entries carry paths only; the caller knows the entity and the
/// component and fills them in.
pub fn resolve<T: Serialize + DeserializeOwned>(
    record: &Record,
    references: &mut dyn ReferenceReader,
) -> Result<(T, Vec<ReportEntry>), RecordError> {
    let mut tree = Tree::new();
    let mut recalled = Recall {
        inner: references,
        seen: HashMap::new(),
    };
    let value: T = read_watched(record, &mut recalled, &mut tree)?;
    // The value written again gives the names the code uses today — what the
    // report speaks in. Its entities are written back as the references the
    // save used for them, so an entity keying a map still finds its entry.
    let canonical = to_record(&value, &mut recalled)?;
    let mut entries = Vec::new();
    walk(&tree, 0, record, &canonical, "", &mut entries);
    Ok((value, entries))
}

/// Resolves references through the caller's reader, and remembers which
/// reference each entity came from so writing the value again says the same.
struct Recall<'r> {
    inner: &'r mut dyn ReferenceReader,
    seen: HashMap<EntityId, EntityRef>,
}

impl ReferenceReader for Recall<'_> {
    fn read_entity(&mut self, reference: EntityRef) -> Result<EntityId, RecordError> {
        let entity = self.inner.read_entity(reference)?;
        self.seen.insert(entity, reference);
        Ok(entity)
    }
}

impl ReferenceWriter for Recall<'_> {
    fn write_entity(&mut self, entity: EntityId) -> EntityRef {
        self.seen
            .get(&entity)
            .copied()
            .unwrap_or(EntityRef::Outside)
    }
}

/// `path` extended by a field or variant name.
fn join(path: &str, name: &str) -> String {
    if path.is_empty() {
        name.to_owned()
    } else {
        format!("{path}.{name}")
    }
}

fn entry(path: &str, kind: ReportKind) -> ReportEntry {
    ReportEntry {
        entity: None,
        component: None,
        path: path.to_owned(),
        kind,
    }
}

/// The value itself, past the optional and newtype wrappers the reader went
/// through without a step of its own.
fn unwrapped(mut record: &Record) -> &Record {
    loop {
        match record {
            Record::Some(inner) | Record::Newtype { value: inner, .. } => record = inner,
            _ => return record,
        }
    }
}

/// The named fields of a struct, of a struct variant's payload, or of a map
/// keyed by strings.
fn named(record: &Record) -> Vec<(&str, &Record)> {
    match record {
        Record::Struct { fields, .. }
        | Record::Variant {
            payload: VariantPayload::Struct(fields),
            ..
        } => fields
            .iter()
            .map(|(name, value)| (name.as_str(), value))
            .collect(),
        Record::Map(entries) => entries
            .iter()
            .filter_map(|(key, value)| match key {
                Record::Str(name) => Some((name.as_str(), value)),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The elements of a sequence, or of a tuple variant's payload.
fn elements(record: &Record) -> &[Record] {
    match record {
        Record::Seq(items)
        | Record::TupleStruct { fields: items, .. }
        | Record::Variant {
            payload: VariantPayload::Tuple(items),
            ..
        } => items,
        _ => &[],
    }
}

/// Whether a value the save wrote resembles one the read produced: the same
/// value, allowing for the widths numbers were read at, for fields one side
/// has and the other not, and for references compared by kind.
fn resembles(written: &Record, read: &Record) -> bool {
    let written = unwrapped(written);
    let read = unwrapped(read);
    let number = |record: &Record| match *record {
        Record::I64(v) => Some(v as f64),
        Record::U64(v) => Some(v as f64),
        Record::F32(v) => Some(f64::from(v)),
        Record::F64(v) => Some(v),
        _ => None,
    };
    if let (Some(a), Some(b)) = (number(written), number(read)) {
        return a == b || (a.is_nan() && b.is_nan());
    }
    match (written, read) {
        (Record::Entity(_), Record::Entity(_)) | (Record::Asset(_), Record::Asset(_)) => true,
        (Record::Variant { variant: a, .. }, Record::Variant { variant: b, .. }) => a == b,
        _ => {
            let (a, b) = (elements(written), elements(read));
            if !a.is_empty() || !b.is_empty() {
                return a.len() == b.len() && a.iter().zip(b).all(|(a, b)| resembles(a, b));
            }
            let (a, b) = (named(written), named(read));
            if !a.is_empty() || !b.is_empty() {
                return a.iter().all(|(name, value)| {
                    b.iter()
                        .find(|(other, _)| other == name)
                        .is_none_or(|(_, other)| resembles(value, other))
                });
            }
            written == read
        }
    }
}

/// Whether two map keys name the same entry, numbers by value.
fn same_key(a: &Record, b: &Record) -> bool {
    let number = |record: &Record| match *record {
        Record::I64(v) => Some(i128::from(v)),
        Record::U64(v) => Some(i128::from(v)),
        _ => None,
    };
    match (number(a), number(b)) {
        (Some(a), Some(b)) => a == b,
        _ => a == b,
    }
}

/// A map key as a path step.
fn key_label(key: &Record) -> String {
    match key {
        Record::I64(v) => format!("[{v}]"),
        Record::U64(v) => format!("[{v}]"),
        Record::Str(v) => format!("[{v:?}]"),
        Record::Char(v) => format!("[{v:?}]"),
        Record::Bool(v) => format!("[{v}]"),
        Record::Variant { variant, .. } => format!("[{variant}]"),
        Record::Entity(EntityRef::Id(id)) => format!("[#{:x}]", id.to_bits()),
        _ => "[?]".to_owned(),
    }
}

/// Reports what the read noted at `node`, naming places by `read` — the
/// value it produced, written again. `written` is what the save held there:
/// the tree mirrors it step for step.
fn walk(
    tree: &Tree,
    node: usize,
    written: &Record,
    read: &Record,
    path: &str,
    entries: &mut Vec<ReportEntry>,
) {
    let Some(here) = tree.nodes.get(node) else {
        return;
    };
    let written = unwrapped(written);
    let read = unwrapped(read);

    let mut declared: Option<&'static [&'static str]> = None;
    let mut positional = false;
    for event in &here.events {
        match event {
            Event::Widened => entries.push(entry(path, ReportKind::Widened)),
            Event::Ignored => entries.push(entry(path, ReportKind::Dropped)),
            Event::Fields(fields) => declared = Some(fields),
            Event::Positional => positional = true,
        }
    }

    if positional {
        walk_positional(tree, here, written, read, path, entries);
        return;
    }

    let fields: Vec<(&str, usize)> = here
        .children
        .iter()
        .filter_map(|(step, child)| match step {
            Step::Field(name) => Some((name.as_str(), *child)),
            _ => None,
        })
        .collect();
    if !fields.is_empty() || declared.is_some() {
        walk_fields(tree, &fields, declared, written, read, path, entries);
    }

    let keyed: Vec<(&Record, usize)> = here
        .children
        .iter()
        .filter_map(|(step, child)| match step {
            Step::Entry(key) => Some((key, *child)),
            _ => None,
        })
        .collect();
    if !keyed.is_empty() {
        walk_entries(tree, &keyed, written, read, path, entries);
    }

    for (step, child) in &here.children {
        match step {
            Step::Index(index) => {
                if let (Some(was), Some(item)) =
                    (elements(written).get(*index), elements(read).get(*index))
                {
                    walk(
                        tree,
                        *child,
                        was,
                        item,
                        &format!("{path}[{index}]"),
                        entries,
                    );
                }
            }
            Step::Variant(name) => {
                walk_variant(tree, *child, name, written, read, path, entries);
            }
            Step::Field(_) | Step::Entry(_) | Step::KeyOf => {}
        }
    }
}

/// A struct the save wrote as a sequence: its elements are its fields in
/// declaration order, and the fields past the last element took defaults.
fn walk_positional(
    tree: &Tree,
    here: &Node,
    written: &Record,
    read: &Record,
    path: &str,
    entries: &mut Vec<ReportEntry>,
) {
    let fields = named(read);
    let items = elements(written);
    let mut count = 0;
    for (step, child) in &here.children {
        if let Step::Index(index) = step {
            count = count.max(index + 1);
            if let (Some(was), Some((name, value))) = (items.get(*index), fields.get(*index)) {
                walk(tree, *child, was, value, &join(path, name), entries);
            }
        }
    }
    for (name, _) in fields.iter().skip(count) {
        entries.push(entry(&join(path, name), ReportKind::Defaulted));
    }
}

/// The entries of a map keyed by something other than names.
///
/// An entry whose key reads back as written is paired with it directly. A
/// key the read adapted — a renamed variant, say — no longer equals any key
/// written back, so the entries left over on both sides are paired in turn;
/// and a written entry left with no counterpart was merged into another by
/// that adaptation, its value lost.
fn walk_entries(
    tree: &Tree,
    keyed: &[(&Record, usize)],
    written: &Record,
    read: &Record,
    path: &str,
    entries: &mut Vec<ReportEntry>,
) {
    let written_value = |key: &Record| match written {
        Record::Map(pairs) => pairs.iter().find(|(known, _)| known == key).map(|(_, v)| v),
        _ => None,
    };
    let read_entries: &[(Record, Record)] = match read {
        Record::Map(read_entries) => read_entries,
        _ => &[],
    };
    let mut used = vec![false; read_entries.len()];
    let mut unmatched = Vec::new();
    for &(key, child) in keyed {
        let here = format!("{path}{}", key_label(key));
        let found = read_entries
            .iter()
            .enumerate()
            .position(|(index, (known, _))| !used[index] && same_key(known, key));
        match found {
            Some(index) => {
                used[index] = true;
                if let Some(was) = written_value(key) {
                    walk(tree, child, was, &read_entries[index].1, &here, entries);
                }
            }
            None => unmatched.push((key, here, child)),
        }
    }
    let mut free = (0..read_entries.len()).filter(|index| !used[*index]);
    for (key, here, child) in unmatched {
        match (free.next(), written_value(key)) {
            (Some(index), Some(was)) => {
                walk(tree, child, was, &read_entries[index].1, &here, entries);
            }
            (Some(_), None) => {}
            (None, _) => entries.push(entry(&here, ReportKind::Dropped)),
        }
    }
}

/// A variant the save wrote as `written`, read as whatever `read` holds.
fn walk_variant(
    tree: &Tree,
    child: usize,
    written_name: &str,
    written: &Record,
    read: &Record,
    path: &str,
    entries: &mut Vec<ReportEntry>,
) {
    // What the variant carried in the save, in whichever form it was written.
    let carried: Record = match written {
        Record::Variant { payload, .. } => match payload {
            VariantPayload::Unit => Record::Unit,
            VariantPayload::Newtype(inner) => (**inner).clone(),
            VariantPayload::Tuple(items) => Record::Seq(items.clone()),
            VariantPayload::Struct(fields) => Record::Struct {
                name: String::new(),
                fields: fields.clone(),
            },
        },
        Record::Map(pairs) if pairs.len() == 1 => pairs[0].1.clone(),
        _ => Record::Unit,
    };
    let (name, payload) = match read {
        Record::Variant {
            variant, payload, ..
        } => (variant.as_str(), Some(payload)),
        // A unit variant a self-describing format wrote as its name.
        Record::Str(name) => (name.as_str(), None),
        _ => return,
    };
    let here = join(path, name);
    if written_name != name {
        entries.push(entry(
            &here,
            ReportKind::Renamed {
                from: written_name.to_owned(),
            },
        ));
    }
    match payload {
        Some(VariantPayload::Newtype(inner)) => {
            walk(tree, child, &carried, inner, &here, entries);
        }
        Some(VariantPayload::Unit) | None => {
            walk(tree, child, &carried, &Record::Unit, &here, entries);
        }
        Some(_) => walk(tree, child, &carried, read, &here, entries),
    }
}

/// The fields the save held at a struct, against the fields the value has.
///
/// A field the save held and the code skipped was dropped (the reader noted
/// it). A field read under a name the value does not use was read through an
/// alias: it is paired with the value's field it belongs to. A field of the
/// value nothing in the save filled was defaulted — said only where the reader
/// read a struct, since a map has no fields to miss.
fn walk_fields(
    tree: &Tree,
    written: &[(&str, usize)],
    declared: Option<&'static [&'static str]>,
    saved: &Record,
    read: &Record,
    path: &str,
    entries: &mut Vec<ReportEntry>,
) {
    let saved_fields = named(saved);
    let saved_value = |name: &str| {
        saved_fields
            .iter()
            .find(|(known, _)| *known == name)
            .map(|(_, value)| *value)
    };
    let read_fields = named(read);
    let current: Vec<&str> = read_fields.iter().map(|(name, _)| *name).collect();
    let ignored = |node: usize| {
        tree.nodes
            .get(node)
            .is_some_and(|node| node.events.iter().any(|e| matches!(e, Event::Ignored)))
    };

    let mut filled: Vec<&str> = Vec::new();
    let mut through_alias: Vec<(&str, usize)> = Vec::new();
    for &(name, node) in written {
        if ignored(node) {
            entries.push(entry(&join(path, name), ReportKind::Dropped));
        } else if current.contains(&name) {
            filled.push(name);
        } else {
            through_alias.push((name, node));
        }
    }

    // A name read that pairs with no field the value still names has nothing
    // to be reported against, and lost nothing the reader did not note.
    let mut renamed: Vec<(&str, &str, usize)> = Vec::new();
    if let Some(declared) = declared {
        for (alias, node) in through_alias {
            if let Some(field) =
                owner_of(alias, saved_value(alias), declared, &read_fields, &filled)
            {
                filled.push(field);
                renamed.push((field, alias, node));
            }
        }
    }

    for (name, value) in &read_fields {
        let here = join(path, name);
        if let Some((_, alias, node)) = renamed.iter().find(|(field, _, _)| field == name) {
            entries.push(entry(
                &here,
                ReportKind::Renamed {
                    from: (*alias).to_owned(),
                },
            ));
            if let Some(was) = saved_value(alias) {
                walk(tree, *node, was, value, &here, entries);
            }
        } else if let Some((_, node)) = written.iter().find(|(written, _)| written == name) {
            if let Some(was) = saved_value(name) {
                walk(tree, *node, was, value, &here, entries);
            }
        } else if declared.is_some() && !filled.contains(name) {
            entries.push(entry(&here, ReportKind::Defaulted));
        }
    }
}

/// Which of the value's fields `alias` is another name for.
///
/// serde lists a struct's accepted names field by field, in declaration
/// order, each field's own names sorted. So an alias sits between the field
/// names around it and belongs to one of those two: the earlier one if it
/// sorts after it, the later one if it sorts before it. A field whose name
/// is not among the accepted ones at all is read under another name and can
/// own any alias. Where several remain, the one whose value the save's value
/// resembles is the owner.
fn owner_of<'a>(
    alias: &str,
    written: Option<&Record>,
    declared: &[&str],
    fields: &[(&'a str, &Record)],
    filled: &[&str],
) -> Option<&'a str> {
    let position = |field: &str| declared.iter().position(|name| *name == field);
    let at = position(alias)?;
    let open = |field: &str| !filled.contains(&field);

    let before = fields
        .iter()
        .filter(|(field, _)| position(field).is_some_and(|p| p < at))
        .max_by_key(|(field, _)| position(field))
        .filter(|(field, _)| alias > *field && open(field));
    let after = fields
        .iter()
        .filter(|(field, _)| position(field).is_some_and(|p| p > at))
        .min_by_key(|(field, _)| position(field))
        .filter(|(field, _)| alias < *field && open(field));
    let elsewhere = fields
        .iter()
        .filter(|(field, _)| position(field).is_none() && open(field));

    let candidates: Vec<&(&'a str, &Record)> =
        before.into_iter().chain(after).chain(elsewhere).collect();
    match candidates.as_slice() {
        [] => None,
        [only] => Some(only.0),
        several => {
            let resembling = written
                .and_then(|written| several.iter().find(|(_, value)| resembles(written, value)));
            Some(resembling.unwrap_or(&several[0]).0)
        }
    }
}
