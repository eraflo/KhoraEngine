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

//! Any serde value as a [`ScriptValue`], and back.
//!
//! A behavior's state companion holds what the engine knows of a running
//! instance — its timers, a suspended sequence, a recorded fault — in fields
//! of any value. Those are engine types with serde derives; this bridge
//! writes them as script values a column can hold, and reads them back:
//! a struct becomes a `Struct` of its fields by name, a sequence an `Array`,
//! an absent `Option` `Null` and a present one its value, an enum variant a
//! one-field `Struct` named by the variant, and an [`EntityId`] — its serde
//! name `khora.EntityId` — a [`ScriptValue::Entity`], so the references a
//! save remaps are found wherever they sit.
//!
//! [`EntityId`]: crate::ecs::entity::EntityId

use serde::de::DeserializeOwned;
use serde::Serialize;

use super::ScriptValue;

mod de;
mod ser;
#[cfg(test)]
mod tests;

/// The serde name an [`EntityId`](crate::ecs::entity::EntityId) is written
/// under.
const ENTITY_NAME: &str = "khora.EntityId";

/// Why a value could not be written as a [`ScriptValue`], or read from one.
#[derive(Debug, Clone, PartialEq)]
pub struct ValueSerdeError(pub String);

impl std::fmt::Display for ValueSerdeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ValueSerdeError {}

/// `value`, written as a [`ScriptValue`].
pub fn to_value<T: Serialize + ?Sized>(value: &T) -> Result<ScriptValue, ValueSerdeError> {
    value.serialize(ser::Writer)
}

/// A `T` read from `value`.
pub fn from_value<T: DeserializeOwned>(value: &ScriptValue) -> Result<T, ValueSerdeError> {
    T::deserialize(de::Reader(value))
}
