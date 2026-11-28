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

//! The saved forms of an [`AssetRef`] of any kind: a record, a snapshot's
//! bytes, the inspector's JSON.

use khora_core::asset::{Asset, AssetUUID, LocalSource};
use khora_core::ecs::entity::EntityId;
use serde::{Deserialize, Serialize};

use super::AssetRef;
use crate::assets::SavedLocalSource;
use crate::ecs::{AnyVec, Component, World};
use crate::scene::positional::{from_positional, to_positional};
use crate::scene::record::{
    Record, RecordError, ReferenceReader, ReferenceWriter, ReportEntry, VariantPayload,
};
use crate::scene::schema::{fingerprint_of, schema_of};
use crate::scene::{StagedComponent, StagedValue};

/// The variant of a reference to a project asset.
const ASSET: &str = "Asset";

/// The variant of a reference to a value made in code.
const MADE: &str = "Made";

/// An asset reference as a snapshot holds it: a local source by the bytes
/// its own positional form wrote.
#[derive(Serialize, Deserialize)]
#[serde(rename = "AssetRef")]
enum AssetRefSnapshot {
    Asset(AssetUUID),
    Made(AssetUUID),
    Local(Vec<u8>),
}

/// Writes row `row` of an `AssetRef<A>` column as a record named `name`.
///
/// # Errors
/// When the reference's local source cannot be written as a record.
pub(crate) fn to_record<A>(
    name: &str,
    column: &dyn AnyVec,
    row: usize,
    references: &mut dyn ReferenceWriter,
) -> Result<Record, RecordError>
where
    A: Asset,
    A::LocalSource: SavedLocalSource<A>,
{
    let (variant, payload) = match <AssetRef<A> as Component>::clone_from_column(column, row) {
        AssetRef::Asset(uuid) => (
            ASSET,
            VariantPayload::Newtype(Box::new(Record::Asset(uuid))),
        ),
        AssetRef::Made(uuid) => (MADE, VariantPayload::Newtype(Box::new(Record::Asset(uuid)))),
        AssetRef::Local(source) => (local_variant::<A>(), source.to_payload(references)?),
    };
    Ok(Record::Variant {
        enum_name: name.to_owned(),
        variant: variant.to_owned(),
        payload,
    })
}

/// Reads an `AssetRef<A>` back from a record, with what the read adapted.
///
/// A local source's adaptations are reported under its variant's name.
///
/// # Errors
/// When the record names no form of the reference, or its form does not read.
pub(crate) fn stage<A>(
    record: &Record,
    references: &mut dyn ReferenceReader,
) -> Result<StagedComponent, RecordError>
where
    A: Asset,
    A::LocalSource: SavedLocalSource<A>,
{
    let (variant, carried) = variant_of(record)?;
    let (reference, report) = match variant {
        ASSET => (AssetRef::<A>::Asset(uuid_in(&carried)?), Vec::new()),
        MADE => (AssetRef::<A>::Made(uuid_in(&carried)?), Vec::new()),
        local if local == local_variant::<A>() => {
            let (source, mut report) = A::LocalSource::from_payload(&carried, references)?;
            for entry in &mut report {
                entry.path = if entry.path.is_empty() {
                    local.to_owned()
                } else {
                    format!("{local}.{}", entry.path)
                };
            }
            (AssetRef::Local(source), report)
        }
        other => {
            return Err(RecordError(format!(
                "no asset reference form `{other}`: `{ASSET}`, `{MADE}` or `{}`",
                local_variant::<A>()
            )))
        }
    };
    Ok(staged(reference, report))
}

/// Writes row `row` of an `AssetRef<A>` column by position, for a snapshot.
///
/// # Errors
/// When the reference's local source cannot be written by position.
pub(crate) fn to_snapshot<A>(
    column: &dyn AnyVec,
    row: usize,
    out: &mut Vec<u8>,
    references: &mut dyn ReferenceWriter,
) -> Result<(), RecordError>
where
    A: Asset,
    A::LocalSource: SavedLocalSource<A>,
{
    let snapshot = match <AssetRef<A> as Component>::clone_from_column(column, row) {
        AssetRef::Asset(uuid) => AssetRefSnapshot::Asset(uuid),
        AssetRef::Made(uuid) => AssetRefSnapshot::Made(uuid),
        AssetRef::Local(source) => {
            let mut bytes = Vec::new();
            source.to_positional(&mut bytes, references)?;
            AssetRefSnapshot::Local(bytes)
        }
    };
    to_positional(&snapshot, out, references)
}

/// Reads an `AssetRef<A>` back from the bytes a snapshot holds for it.
///
/// # Errors
/// When the bytes hold no reference of this kind, consumed exactly.
pub(crate) fn stage_snapshot<A>(
    bytes: &[u8],
    references: &mut dyn ReferenceReader,
) -> Result<StagedComponent, RecordError>
where
    A: Asset,
    A::LocalSource: SavedLocalSource<A>,
{
    let reference = match from_positional::<AssetRefSnapshot>(bytes, references)? {
        AssetRefSnapshot::Asset(uuid) => AssetRef::<A>::Asset(uuid),
        AssetRefSnapshot::Made(uuid) => AssetRef::Made(uuid),
        AssetRefSnapshot::Local(bytes) => {
            AssetRef::Local(A::LocalSource::from_positional(&bytes, references)?)
        }
    };
    Ok(staged(reference, Vec::new()))
}

/// Returns the fingerprint of an `AssetRef<A>`'s positional form, and whether it is complete.
pub(crate) fn schema<A>() -> (u64, bool)
where
    A: Asset,
    A::LocalSource: SavedLocalSource<A>,
{
    let shape = schema_of::<AssetRefSnapshot>();
    let (local, local_complete) = <A::LocalSource as SavedLocalSource<A>>::schema();
    let complete = shape.is_complete() && local_complete;
    if !complete {
        log::warn!(
            "the schema of an asset reference could not be traced in full: it is never snapshotted"
        );
    }
    (fingerprint_of(&format!("{shape};{local:016x}")), complete)
}

/// Returns the inspector's JSON of `entity`'s `AssetRef<A>`: one key, its form's name.
pub(crate) fn to_json<A>(world: &World, entity: EntityId) -> Option<serde_json::Value>
where
    A: Asset,
    A::LocalSource: SavedLocalSource<A>,
{
    let (variant, value) = match world.get::<AssetRef<A>>(entity)? {
        AssetRef::Asset(uuid) => (ASSET, serde_json::to_value(uuid).ok()?),
        AssetRef::Made(uuid) => (MADE, serde_json::to_value(uuid).ok()?),
        AssetRef::Local(source) => (local_variant::<A>(), source.to_json()?),
    };
    let mut form = serde_json::Map::new();
    form.insert(variant.to_owned(), value);
    Some(serde_json::Value::Object(form))
}

/// Reads the inspector's JSON back into `entity`'s `AssetRef<A>`, added if missing.
///
/// # Errors
/// When the JSON names no form of the reference, or its form does not read.
pub(crate) fn from_json<A>(
    world: &mut World,
    entity: EntityId,
    value: &serde_json::Value,
) -> Result<(), String>
where
    A: Asset,
    A::LocalSource: SavedLocalSource<A>,
{
    let reference = reference_from_json::<A>(value)?;
    if !world.set_component(entity, reference.clone()) {
        world
            .add_component(entity, reference)
            .map_err(|error| format!("{error:?}"))?;
    }
    Ok(())
}

/// Returns whether row `row` of an `AssetRef<A>` column names a value made in code.
pub(crate) fn is_made<A: Asset>(column: &dyn AnyVec, row: usize) -> bool {
    <AssetRef<A> as Component>::clone_from_column(column, row).is_made()
}

/// Removes `entity`'s `AssetRef<A>`, leaving its other components.
///
/// # Errors
/// When the entity holds no such reference, with the world's reason.
pub(crate) fn remove<A: Asset>(world: &mut World, entity: EntityId) -> Result<(), String> {
    world
        .remove_component::<AssetRef<A>>(entity)
        .map(|_| ())
        .map_err(|error| format!("{error:?}"))
}

/// Returns the variant name of `A`'s local form.
fn local_variant<A: Asset>() -> &'static str {
    <A::LocalSource as LocalSource<A>>::VARIANT
}

/// Returns the reference the inspector's JSON `value` names.
fn reference_from_json<A>(value: &serde_json::Value) -> Result<AssetRef<A>, String>
where
    A: Asset,
    A::LocalSource: SavedLocalSource<A>,
{
    let Some((variant, inner)) = value
        .as_object()
        .filter(|form| form.len() == 1)
        .and_then(|form| form.iter().next())
    else {
        return Err(format!(
            "an asset reference is one key, its form: `{ASSET}`, `{MADE}` or `{}`",
            local_variant::<A>()
        ));
    };
    let uuid = || serde_json::from_value::<AssetUUID>(inner.clone()).map_err(|e| e.to_string());
    match variant.as_str() {
        ASSET => uuid().map(AssetRef::Asset),
        MADE => uuid().map(AssetRef::Made),
        local if local == local_variant::<A>() => {
            A::LocalSource::from_json(inner).map(AssetRef::Local)
        }
        other => Err(format!("no asset reference form `{other}`")),
    }
}

/// Returns the variant a record names and what it carries, as written or as
/// a self-describing format reads it (a map keyed by the variant's name).
fn variant_of(record: &Record) -> Result<(&str, Record), RecordError> {
    match record {
        Record::Variant {
            variant, payload, ..
        } => Ok((variant, carried(payload))),
        Record::Map(entries) if entries.len() == 1 => match &entries[0] {
            (Record::Str(variant), value) => Ok((variant, value.clone())),
            _ => Err(RecordError(
                "an asset reference keyed by a non-name".to_owned(),
            )),
        },
        _ => Err(RecordError(
            "an asset reference is recorded as one of its forms".to_owned(),
        )),
    }
}

/// Returns what a variant carries as a plain record.
fn carried(payload: &VariantPayload) -> Record {
    match payload {
        VariantPayload::Newtype(inner) => (**inner).clone(),
        VariantPayload::Struct(fields) => Record::Struct {
            name: String::new(),
            fields: fields.clone(),
        },
        VariantPayload::Tuple(items) => Record::Seq(items.clone()),
        VariantPayload::Unit => Record::Unit,
    }
}

/// Returns the uuid a form carries.
fn uuid_in(carried: &Record) -> Result<AssetUUID, RecordError> {
    match carried {
        Record::Asset(uuid) => Ok(*uuid),
        _ => Err(RecordError(
            "an asset reference without its uuid".to_owned(),
        )),
    }
}

/// Returns `reference` staged for a load, with what reading it adapted.
fn staged<A: Asset>(reference: AssetRef<A>, report: Vec<ReportEntry>) -> StagedComponent {
    StagedComponent {
        component: Box::new(StagedValue(reference)),
        report,
    }
}
