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

//! The `RelayoutReport`: what giving a declared component new fields did to its rows.

use super::lost_value::{LossCause, LostValue};

/// What a [`World::relayout`](super::World::relayout) did to the rows it rebuilt.
///
/// Every value a live row held and does not hold as it was is one of
/// [`lost`](Self::lost); a value kept, or an `int` kept as the `float` naming
/// it exactly, is in none of its lists.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RelayoutReport {
    /// The component relaid out, by name.
    pub component: String,
    /// Fields the old layout had and the new one does not: their values are gone.
    pub dropped: Vec<String>,
    /// Fields every live row holds the default of: new ones, and those whose
    /// new type kept no live row's value.
    pub defaulted: Vec<String>,
    /// Each value a live row did not keep, by entity and field, in page order.
    pub lost: Vec<LostValue>,
}

impl RelayoutReport {
    /// Returns whether the relayout lost no value a row held.
    pub fn lost_nothing(&self) -> bool {
        self.lost.is_empty()
    }

    /// Returns the values lost in `field` for `cause`, in page order.
    pub fn lost_in<'a>(
        &'a self,
        field: &'a str,
        cause: LossCause,
    ) -> impl Iterator<Item = &'a LostValue> + 'a {
        self.lost
            .iter()
            .filter(move |lost| lost.field == field && lost.cause == cause)
    }
}

#[cfg(test)]
mod tests {
    use khora_core::ecs::entity::EntityId;
    use khora_core::script::ScriptValue;

    use super::*;

    fn lost(field: &str, cause: LossCause) -> LostValue {
        LostValue {
            entity: EntityId {
                index: 1,
                generation: 0,
            },
            field: field.to_owned(),
            was: ScriptValue::Int(16_777_217),
            now: Some(ScriptValue::Float(0.0)),
            cause,
        }
    }

    /// A report with no lost value lost nothing, whatever its field lists say.
    #[test]
    fn a_report_without_lost_values_lost_nothing() {
        let report = RelayoutReport {
            component: "Health".to_owned(),
            dropped: Vec::new(),
            defaulted: vec!["armor".to_owned()],
            lost: Vec::new(),
        };

        assert!(report.lost_nothing());
    }

    /// The values lost are read back by field and cause.
    #[test]
    fn lost_in_reads_the_values_of_one_field_and_cause() {
        let report = RelayoutReport {
            component: "Health".to_owned(),
            dropped: vec!["old".to_owned()],
            defaulted: Vec::new(),
            lost: vec![
                lost("hp", LossCause::Inexact),
                lost("old", LossCause::FieldDropped),
                lost("hp", LossCause::Retyped),
            ],
        };

        assert!(!report.lost_nothing());
        assert_eq!(
            report.lost_in("hp", LossCause::Inexact).collect::<Vec<_>>(),
            vec![&lost("hp", LossCause::Inexact)]
        );
        assert_eq!(report.lost_in("speed", LossCause::Inexact).count(), 0);
    }
}
