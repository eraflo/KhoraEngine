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

//! The game's physics settings: the `physics` section of its settings.

use std::time::Duration;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The `physics` section of a game's settings.
///
/// A game setting, never a GORNA lever: the simulation's result depends on
/// its step, so no budget may change it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicsSettings {
    /// `physics.step`: the simulated time one physics step advances, written
    /// in seconds. Always positive.
    pub step: Duration,
}

impl PhysicsSettings {
    /// The step a game that sets none runs at: a sixtieth of a second.
    pub const DEFAULT_STEP: Duration = Duration::from_nanos(16_666_667);

    /// The shortest step a game may set: half a millisecond, 2000 steps a
    /// simulated second. A frame runs as many physics steps as its time holds,
    /// so a step a settings file made tiny would freeze the engine.
    pub const MIN_STEP: Duration = Duration::from_micros(500);

    /// Whether these are the settings a game that sets nothing has.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

impl Default for PhysicsSettings {
    fn default() -> Self {
        Self {
            step: Self::DEFAULT_STEP,
        }
    }
}

/// The section as it is written: the step in seconds, absent for the default.
#[derive(Serialize, Deserialize)]
struct Written {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    step: Option<f64>,
}

impl Serialize for PhysicsSettings {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Written {
            step: Some(self.step.as_secs_f64()),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PhysicsSettings {
    /// A step that is not a positive, representable number of seconds, or
    /// one shorter than [`MIN_STEP`](Self::MIN_STEP), is refused here, at the
    /// file: no step the scheduler could not run reaches it.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let written = Written::deserialize(deserializer)?;
        let Some(seconds) = written.step else {
            return Ok(Self::default());
        };
        if !(seconds.is_finite() && seconds > 0.0) {
            return Err(serde::de::Error::custom(format!(
                "`physics.step` is {seconds} s; it must be a positive number of seconds"
            )));
        }
        let step = Duration::try_from_secs_f64(seconds).map_err(|_| {
            serde::de::Error::custom(format!(
                "`physics.step` is {seconds} s, more than a step can be"
            ))
        })?;
        if step < Self::MIN_STEP {
            return Err(serde::de::Error::custom(format!(
                "`physics.step` is {seconds} s, shorter than the {} s a step may be",
                Self::MIN_STEP.as_secs_f64()
            )));
        }
        Ok(Self { step })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sixtieth of a second, within a microsecond: `1/60` has no exact
    /// `Duration`.
    fn is_a_sixtieth(step: Duration) -> bool {
        let sixtieth = Duration::from_secs_f64(1.0 / 60.0);
        let gap = step.max(sixtieth) - step.min(sixtieth);
        gap < Duration::from_micros(1)
    }

    /// 1/128 s, exact in binary, so the comparisons below need no tolerance.
    const A_128TH: Duration = Duration::from_nanos(7_812_500);

    /// A game that sets nothing steps its physics sixty times a simulated
    /// second, as the engine always has.
    #[test]
    fn the_default_physics_step_is_a_sixtieth_of_a_second() {
        let step = PhysicsSettings::default().step;

        assert!(is_a_sixtieth(step), "the default step is 1/60 s: {step:?}");
    }

    /// **`physics.step` is written in seconds.** A settings file says
    /// `"step": 0.0078125`, not a `Duration`'s `{ secs, nanos }`.
    #[test]
    fn the_physics_step_is_read_in_seconds() {
        let read = serde_json::from_str::<PhysicsSettings>(r#"{ "step": 0.0078125 }"#);

        match read {
            Ok(settings) => assert_eq!(settings.step, A_128TH),
            Err(error) => panic!("a step in seconds is read: {error}"),
        }
    }

    /// A physics section that names no step keeps the default one.
    #[test]
    fn a_physics_section_without_a_step_keeps_the_default_step() {
        let read = serde_json::from_str::<PhysicsSettings>("{}");

        match read {
            Ok(settings) => assert_eq!(settings, PhysicsSettings::default()),
            Err(error) => panic!("an empty section is read: {error}"),
        }
    }

    /// A step shorter than half a millisecond is refused: a frame runs as many
    /// steps as its time holds, and a tiny one would freeze the engine. The
    /// floor itself is read.
    #[test]
    fn a_step_shorter_than_the_floor_is_refused() {
        let floor = serde_json::from_str::<PhysicsSettings>(r#"{ "step": 0.0005 }"#);
        assert!(floor.is_ok(), "the floor itself is a step: {floor:?}");

        for tiny in [r#"{ "step": 0.0001 }"#, r#"{ "step": 1e-9 }"#] {
            let read = serde_json::from_str::<PhysicsSettings>(tiny);
            assert!(read.is_err(), "{tiny} is refused, not read as {read:?}");
        }
    }

    /// What is written is what is read: the step goes back out in seconds.
    #[test]
    fn the_physics_step_is_written_in_seconds() {
        let written = serde_json::to_value(PhysicsSettings { step: A_128TH });

        match written {
            Ok(value) => assert_eq!(value, serde_json::json!({ "step": 0.0078125 })),
            Err(error) => panic!("the settings are written: {error}"),
        }
    }

    /// **Only a positive step is read.** A zero step would be no step at all,
    /// and a negative one has no meaning; neither may reach the scheduler,
    /// and neither may panic the read (`Duration::from_secs_f64` does on a
    /// negative number).
    #[test]
    fn only_a_positive_physics_step_is_read() {
        let positive = serde_json::from_str::<PhysicsSettings>(r#"{ "step": 0.0078125 }"#);
        assert!(positive.is_ok(), "a positive step is read: {positive:?}");

        for refused in [
            r#"{ "step": 0 }"#,
            r#"{ "step": 0.0 }"#,
            r#"{ "step": -0.01 }"#,
            r#"{ "step": 1e300 }"#,
        ] {
            let read =
                std::panic::catch_unwind(|| serde_json::from_str::<PhysicsSettings>(refused));
            match read {
                Ok(Err(_)) => {}
                Ok(Ok(settings)) => panic!("{refused} is refused, not read as {settings:?}"),
                Err(_) => panic!("reading {refused} panicked instead of refusing it"),
            }
        }
    }
}
