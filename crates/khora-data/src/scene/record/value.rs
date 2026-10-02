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

//! The record tree.

use khora_core::asset::AssetUUID;
use khora_core::ecs::PersistentId;

/// A value in serde's data model, names included.
#[derive(Debug, Clone, PartialEq)]
pub enum Record {
    /// `()`.
    Unit,
    /// `bool`.
    Bool(bool),
    /// Any signed integer, widened.
    I64(i64),
    /// Any unsigned integer, widened.
    U64(u64),
    /// `f32`, kept at its own width so it reads back bit-exact.
    F32(f32),
    /// `f64`.
    F64(f64),
    /// `char`.
    Char(char),
    /// A string.
    Str(String),
    /// A byte string.
    Bytes(Vec<u8>),
    /// An absent optional.
    None,
    /// A present optional.
    Some(Box<Record>),
    /// A sequence, a tuple or a fixed-size array.
    Seq(Vec<Record>),
    /// A map, as its entries.
    Map(Vec<(Record, Record)>),
    /// A struct with no fields.
    UnitStruct {
        /// Its serde name.
        name: String,
    },
    /// A struct with named fields.
    Struct {
        /// Its serde name.
        name: String,
        /// Its fields, by name, in the order they were written.
        fields: Vec<(String, Record)>,
    },
    /// A tuple struct with more than one field.
    TupleStruct {
        /// Its serde name.
        name: String,
        /// Its fields, by position.
        fields: Vec<Record>,
    },
    /// A tuple struct with exactly one field.
    Newtype {
        /// Its serde name.
        name: String,
        /// The wrapped value.
        value: Box<Record>,
    },
    /// An enum variant.
    Variant {
        /// The enum's serde name.
        enum_name: String,
        /// The variant's name.
        variant: String,
        /// What the variant carries.
        payload: VariantPayload,
    },
    /// A reference to an entity.
    Entity(EntityRef),
    /// A reference to an asset.
    Asset(AssetUUID),
}

/// What an enum variant carries.
#[derive(Debug, Clone, PartialEq)]
pub enum VariantPayload {
    /// Nothing: `A`.
    Unit,
    /// One value: `A(x)`.
    Newtype(Box<Record>),
    /// Several values: `A(x, y)`.
    Tuple(Vec<Record>),
    /// Named fields: `A { x, y }`.
    Struct(Vec<(String, Record)>),
}

/// An entity, as a save refers to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntityRef {
    /// An entity the save records, by its identity.
    Id(PersistentId),
    /// An entity outside what the save records — gone, or never part of it.
    Outside,
}
