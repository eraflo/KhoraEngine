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

//! [`SavedLocalSource`], the saved forms of a value a reference carries itself.

use khora_core::asset::{LocalSource, NoLocalSource};

use crate::scene::record::{
    Record, RecordError, ReferenceReader, ReferenceWriter, ReportEntry, VariantPayload,
};
use crate::scene::schema::fingerprint_of;

/// The forms a local source is saved in: a record, a snapshot's bytes, the inspector's JSON.
pub trait SavedLocalSource<A>: LocalSource<A> {
    /// Writes the source as the payload of its reference's variant.
    ///
    /// # Errors
    /// When the source cannot be written as a record.
    fn to_payload(
        &self,
        references: &mut dyn ReferenceWriter,
    ) -> Result<VariantPayload, RecordError>;

    /// Reads a source back from its variant's payload, with what the read adapted.
    ///
    /// # Errors
    /// When the payload holds no source of this kind.
    fn from_payload(
        payload: &Record,
        references: &mut dyn ReferenceReader,
    ) -> Result<(Self, Vec<ReportEntry>), RecordError>;

    /// Writes the source by position, for a snapshot.
    ///
    /// # Errors
    /// When the source cannot be written by position.
    fn to_positional(
        &self,
        out: &mut Vec<u8>,
        references: &mut dyn ReferenceWriter,
    ) -> Result<(), RecordError>;

    /// Reads a source back from its positional bytes.
    ///
    /// # Errors
    /// When the bytes hold no source of this kind.
    fn from_positional(
        bytes: &[u8],
        references: &mut dyn ReferenceReader,
    ) -> Result<Self, RecordError>;

    /// Returns the fingerprint of the positional form, and whether it is complete.
    fn schema() -> (u64, bool);

    /// Returns the inspector's JSON form, `None` when the source has none.
    fn to_json(&self) -> Option<serde_json::Value>;

    /// Reads a source back from the inspector's JSON form.
    ///
    /// # Errors
    /// When the JSON holds no source of this kind.
    fn from_json(value: &serde_json::Value) -> Result<Self, String>;
}

/// A kind no reference can carry: every saved local form is refused.
impl<A> SavedLocalSource<A> for NoLocalSource {
    fn to_payload(&self, _: &mut dyn ReferenceWriter) -> Result<VariantPayload, RecordError> {
        match *self {}
    }

    fn from_payload(
        _: &Record,
        _: &mut dyn ReferenceReader,
    ) -> Result<(Self, Vec<ReportEntry>), RecordError> {
        Err(no_local_form())
    }

    fn to_positional(
        &self,
        _: &mut Vec<u8>,
        _: &mut dyn ReferenceWriter,
    ) -> Result<(), RecordError> {
        match *self {}
    }

    fn from_positional(_: &[u8], _: &mut dyn ReferenceReader) -> Result<Self, RecordError> {
        Err(no_local_form())
    }

    fn schema() -> (u64, bool) {
        (fingerprint_of("NoLocalSource"), true)
    }

    fn to_json(&self) -> Option<serde_json::Value> {
        match *self {}
    }

    fn from_json(_: &serde_json::Value) -> Result<Self, String> {
        Err(no_local_form().0)
    }
}

/// Returns the refusal of a local form by a kind no reference can carry.
fn no_local_form() -> RecordError {
    RecordError("this kind of asset has no local form: a reference names it by its uuid".to_owned())
}

#[cfg(test)]
mod tests {
    use khora_core::asset::font::Font;
    use khora_core::asset::{AssetUUID, NoLocalSource};
    use khora_core::ecs::entity::EntityId;

    use super::SavedLocalSource;
    use crate::ecs::AssetRef;
    use crate::scene::record::{EntityRef, Record, RecordError, ReferenceReader};

    /// Reads no entity: a local form refused before any entity is read.
    struct NoEntities;

    impl ReferenceReader for NoEntities {
        fn read_entity(&mut self, _: EntityRef) -> Result<EntityId, RecordError> {
            Err(RecordError("no entity here".to_owned()))
        }
    }

    /// A kind no reference can carry refuses a local form, in each of its saved
    /// forms: a record, a snapshot's bytes, the inspector's JSON.
    #[test]
    fn a_kind_without_local_source_refuses_a_local_form() {
        // A font reference has only the forms that name an asset.
        let _: Option<AssetRef<Font>> = Some(AssetRef::Asset(AssetUUID::new()));
        let payload = Record::Struct {
            name: String::new(),
            fields: vec![("name".to_owned(), Record::Str("Inter".to_owned()))],
        };

        assert!(
            <NoLocalSource as SavedLocalSource<Font>>::from_payload(&payload, &mut NoEntities)
                .is_err(),
            "a record"
        );
        assert!(
            <NoLocalSource as SavedLocalSource<Font>>::from_payload(&Record::Unit, &mut NoEntities)
                .is_err(),
            "an empty record"
        );
        assert!(
            <NoLocalSource as SavedLocalSource<Font>>::from_positional(&[], &mut NoEntities)
                .is_err(),
            "no bytes"
        );
        assert!(
            <NoLocalSource as SavedLocalSource<Font>>::from_positional(
                &[1, 2, 3, 4],
                &mut NoEntities
            )
            .is_err(),
            "some bytes"
        );
        assert!(
            <NoLocalSource as SavedLocalSource<Font>>::from_json(&serde_json::json!({
                "Local": { "name": "Inter" }
            }))
            .is_err(),
            "the inspector's JSON"
        );
    }
}
