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

//! The `DeclaredRelayout`: what a script's changed declaration did to the
//! rows stored, told to the log and to `Problems`.

use khora_core::problems::{Problem, ProblemSeverity};
use khora_core::script::ScriptValue;
use khora_data::ecs::{LossCause, LostValue, RelayoutReport};

/// How many entities one problem names before it counts the rest.
const ENTITIES_NAMED: usize = 3;

/// What relaying out one declared component did to its rows, and the module declaring it.
#[derive(Debug, Clone, PartialEq)]
pub struct DeclaredRelayout {
    /// The module declaring the component, by its path under the script root.
    pub module: String,
    /// What the relayout did, every value a row lost included.
    pub report: RelayoutReport,
}

impl DeclaredRelayout {
    /// Returns one warning per field and cause that lost values, located in the module.
    ///
    /// Each names the component, the field, and the entities with the value
    /// each held and the one its row took; the entity too when it is the only one.
    pub fn problems(&self) -> Vec<Problem> {
        self.losses()
            .into_iter()
            .map(|(field, cause, values)| {
                let only = match values.as_slice() {
                    [one] => Some(one.entity),
                    _ => None,
                };
                Problem {
                    note: Some(
                        "the running world holds the new value; a scene saved before the edit still holds the old one"
                            .to_owned(),
                    ),
                    entity: only,
                    component: Some(self.report.component.clone()),
                    field: Some(field.to_owned()),
                    ..Problem::of_file(
                        ProblemSeverity::Warning,
                        self.module.clone(),
                        0,
                        0,
                        self.message(field, cause, &values),
                    )
                }
            })
            .collect()
    }

    /// Logs what the relayout did: a warning per field that lost values, and
    /// what it changed losing nothing.
    pub fn log(&self) {
        let report = &self.report;
        let name = &report.component;
        for (field, cause, values) in self.losses() {
            log::warn!(
                "`{}` (declared in `{}`): {}",
                name,
                self.module,
                self.message(field, cause, &values)
            );
        }
        let lossless_drops: Vec<&String> = report
            .dropped
            .iter()
            .filter(|field| !report.lost.iter().any(|lost| lost.field == **field))
            .collect();
        if !lossless_drops.is_empty() {
            log::info!("`{name}` dropped the fields {lossless_drops:?}, which no row held");
        }
        if !report.defaulted.is_empty() {
            log::info!(
                "`{name}` takes the defaults of its new or retyped fields {:?}",
                report.defaulted
            );
        }
    }

    /// Returns the values lost, by field and cause, in the order first met.
    fn losses(&self) -> Vec<(&str, LossCause, Vec<&LostValue>)> {
        let mut grouped: Vec<(&str, LossCause, Vec<&LostValue>)> = Vec::new();
        for lost in &self.report.lost {
            match grouped
                .iter_mut()
                .find(|(field, cause, _)| *field == lost.field && *cause == lost.cause)
            {
                Some((_, _, values)) => values.push(lost),
                None => grouped.push((lost.field.as_str(), lost.cause, vec![lost])),
            }
        }
        grouped
    }

    /// Says what `values` of `field` were, what their rows hold, and why.
    fn message(&self, field: &str, cause: LossCause, values: &[&LostValue]) -> String {
        let count = values.len();
        let mut rows: Vec<String> = values
            .iter()
            .take(ENTITIES_NAMED)
            .map(|lost| {
                let mut row = format!("entity #{} held {}", lost.entity.index, shown(&lost.was));
                if let Some(now) = &lost.now {
                    row.push_str(&format!(", now {}", shown(now)));
                }
                row
            })
            .collect();
        if count > ENTITIES_NAMED {
            rows.push(format!("and {} more", count - ENTITIES_NAMED));
        }
        format!(
            "`{}` lost {} {} of `{field}`: {}; {}",
            self.report.component,
            count,
            if count == 1 { "value" } else { "values" },
            cause.reason(),
            rows.join("; ")
        )
    }
}

/// Returns `value` as an author writes it in a script.
fn shown(value: &ScriptValue) -> String {
    match value {
        ScriptValue::Bool(v) => v.to_string(),
        ScriptValue::Int(v) => v.to_string(),
        ScriptValue::Float(v) => v.to_string(),
        ScriptValue::Str(v) => format!("{v:?}"),
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use khora_core::ecs::entity::EntityId;

    use super::*;

    fn entity(index: u32) -> EntityId {
        EntityId {
            index,
            generation: 0,
        }
    }

    fn lost(index: u32, field: &str, was: i64, cause: LossCause) -> LostValue {
        LostValue {
            entity: entity(index),
            field: field.to_owned(),
            was: ScriptValue::Int(was),
            now: (cause != LossCause::FieldDropped).then_some(ScriptValue::Float(0.5)),
            cause,
        }
    }

    fn relayout(lost: Vec<LostValue>) -> DeclaredRelayout {
        DeclaredRelayout {
            module: "guard.erg".to_owned(),
            report: RelayoutReport {
                component: "Health".to_owned(),
                dropped: Vec::new(),
                defaulted: Vec::new(),
                lost,
            },
        }
    }

    /// A relayout that lost nothing is no problem.
    #[test]
    fn a_relayout_losing_nothing_is_no_problem() {
        assert!(relayout(Vec::new()).problems().is_empty());
    }

    /// One value lost is one warning naming its entity, its field, the value and the default.
    #[test]
    fn one_value_lost_is_a_warning_naming_its_entity() {
        let problems = relayout(vec![lost(4, "hp", 16_777_217, LossCause::Inexact)]).problems();

        assert_eq!(problems.len(), 1);
        let problem = &problems[0];
        assert_eq!(problem.severity, ProblemSeverity::Warning);
        assert_eq!(problem.module.as_deref(), Some("guard.erg"));
        assert_eq!(problem.component.as_deref(), Some("Health"));
        assert_eq!(problem.field.as_deref(), Some("hp"));
        assert_eq!(problem.entity, Some(entity(4)));
        assert!(
            problem.message.contains("entity #4 held 16777217, now 0.5"),
            "{}",
            problem.message
        );
    }

    /// Values lost in one field for one cause are one warning, counting past the entities it names.
    #[test]
    fn values_of_one_field_and_cause_are_one_warning_counting_the_rest() {
        let problems = relayout(
            (1..=5)
                .map(|index| lost(index, "hp", i64::from(index), LossCause::FieldDropped))
                .chain([lost(9, "armor", 3, LossCause::Retyped)])
                .collect(),
        )
        .problems();

        assert_eq!(problems.len(), 2, "{problems:?}");
        let hp = &problems[0];
        assert_eq!(hp.field.as_deref(), Some("hp"));
        assert_eq!(hp.entity, None, "five entities");
        assert!(hp.message.contains("lost 5 values"), "{}", hp.message);
        assert!(hp.message.contains("and 2 more"), "{}", hp.message);
        assert!(
            !hp.message.contains("now"),
            "a dropped field holds nothing now"
        );
        assert_eq!(problems[1].field.as_deref(), Some("armor"));
    }
}
