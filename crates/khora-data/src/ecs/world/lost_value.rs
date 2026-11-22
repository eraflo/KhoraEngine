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

//! The `LostValue`: one value a row did not keep through a relayout, and why.

use khora_core::ecs::entity::EntityId;
use khora_core::script::ScriptValue;

/// One value a live row held before a relayout and does not hold after it.
#[derive(Debug, Clone, PartialEq)]
pub struct LostValue {
    /// The entity whose row it was.
    pub entity: EntityId,
    /// The field, by its name before the relayout.
    pub field: String,
    /// The value the row held.
    pub was: ScriptValue,
    /// What the row holds in its place: the field's default, `None` when the field is gone.
    pub now: Option<ScriptValue>,
    /// Why the row could not keep it.
    pub cause: LossCause,
}

/// Why a row did not keep a value through a relayout: what a scene load would do with it too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LossCause {
    /// Its field is not in the new declaration: a load drops it the same way.
    FieldDropped,
    /// Its field's new type refuses the value: a `bool` in a field become an
    /// `int`, text in one become an `int?`, an asset of another kind.
    Retyped,
    /// The value holds an `int` a `float` cannot hold exactly (past 2²⁴): a load refuses it too.
    ///
    /// The field's own value, or one inside an optional, an array or a struct.
    Inexact,
    /// Its field's type holds the value, the component does not: a number that
    /// is not finite, in a component a scene keeps.
    Refused,
}

impl LossCause {
    /// Returns how a person reads the cause, completing "the value was lost: …".
    pub fn reason(self) -> &'static str {
        match self {
            Self::FieldDropped => "its field is not in the new declaration",
            Self::Retyped => "its field's new type cannot hold it",
            Self::Inexact => "a `float` cannot hold this `int` exactly",
            Self::Refused => "the component keeps only finite numbers",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every cause reads as its own reason.
    #[test]
    fn every_cause_reads_as_its_own_reason() {
        let reasons = [
            LossCause::FieldDropped,
            LossCause::Retyped,
            LossCause::Inexact,
            LossCause::Refused,
        ]
        .map(LossCause::reason);

        for (at, reason) in reasons.iter().enumerate() {
            assert!(!reason.is_empty());
            assert!(
                !reasons[at + 1..].contains(reason),
                "`{reason}` is given twice"
            );
        }
    }
}
