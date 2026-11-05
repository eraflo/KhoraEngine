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

//! Which families run.
//!
//! The engine has no mode: every family is on until the application switches
//! it off. A request is only a request; it takes effect at the frame boundary,
//! when the engine [`settle`](Activation::settle)s it, so a whole frame — its
//! DataSystems, its Flows, its agents and the DCC — sees one state.

use std::collections::BTreeSet;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use super::gorna::AgentId;

/// What a family does, for the frame in force.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum State {
    /// The family runs: its agent, its systems and its Flows.
    On,
    /// The family does not run.
    Off,
}

/// A request named a family the engine does not know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnknownFamily(pub AgentId);

impl std::fmt::Display for UnknownFamily {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "no family named `{}` is registered", self.0)
    }
}

impl std::error::Error for UnknownFamily {}

/// The families switched on and off: the state requested, the state in force
/// and a generation that moves each time the state in force changes.
///
/// One leaf lock, held only to read or write the two states.
#[derive(Debug)]
pub struct Activation {
    state: RwLock<Switches>,
}

#[derive(Debug, Default)]
struct Switches {
    known: BTreeSet<AgentId>,
    requested_off: BTreeSet<AgentId>,
    off: BTreeSet<AgentId>,
    generation: u64,
}

impl Activation {
    /// Every family in `families` known and on.
    ///
    /// A request naming a family outside `families` is refused.
    pub fn new(families: impl IntoIterator<Item = AgentId>) -> Self {
        Self {
            state: RwLock::new(Switches {
                known: families.into_iter().collect(),
                ..Switches::default()
            }),
        }
    }

    /// Makes `families` known too, each on: a family registered after the
    /// activation was made — an application's own agent — can be switched
    /// from then on. A family already known keeps its state.
    pub fn admit(&self, families: impl IntoIterator<Item = AgentId>) {
        self.write().known.extend(families);
    }

    /// Requests `family` on, from the next [`settle`](Self::settle).
    pub fn activate(&self, family: AgentId) -> Result<(), UnknownFamily> {
        self.request(family, |off| {
            off.remove(&family);
        })
    }

    /// Requests `family` off, from the next [`settle`](Self::settle).
    pub fn deactivate(&self, family: AgentId) -> Result<(), UnknownFamily> {
        self.request(family, |off| {
            off.insert(family);
        })
    }

    /// The state in force of `family`. A family the engine does not know is
    /// never switched, so it is on.
    pub fn state(&self, family: AgentId) -> State {
        if self.read().off.contains(&family) {
            State::Off
        } else {
            State::On
        }
    }

    /// Whether `family` is on, in the state in force.
    pub fn is_active(&self, family: AgentId) -> bool {
        self.state(family) == State::On
    }

    /// The families off, in the state in force.
    pub fn inactive(&self) -> BTreeSet<AgentId> {
        self.read().off.clone()
    }

    /// A number that moves each time the state in force changes, never
    /// otherwise.
    pub fn generation(&self) -> u64 {
        self.read().generation
    }

    /// Makes the requested state the state in force, and says whether that
    /// changed it.
    pub fn settle(&self) -> bool {
        let mut switches = self.write();
        if switches.requested_off == switches.off {
            return false;
        }
        switches.off = switches.requested_off.clone();
        switches.generation += 1;
        true
    }

    fn request(
        &self,
        family: AgentId,
        change: impl FnOnce(&mut BTreeSet<AgentId>),
    ) -> Result<(), UnknownFamily> {
        let mut switches = self.write();
        if !switches.known.contains(&family) {
            log::warn!("a request to switch `{family}`, which is no registered family, is refused");
            return Err(UnknownFamily(family));
        }
        change(&mut switches.requested_off);
        Ok(())
    }

    // The lock guards plain sets that every write leaves whole, so a panic
    // elsewhere while it was held leaves nothing half-written: read through
    // the poison.
    fn read(&self) -> RwLockReadGuard<'_, Switches> {
        self.state
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn write(&self) -> RwLockWriteGuard<'_, Switches> {
        self.state
            .write()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRIPT: AgentId = AgentId::new("script");
    const PHYSICS: AgentId = AgentId::new("physics");
    const AUDIO: AgentId = AgentId::new("audio");

    fn engine() -> Activation {
        Activation::new([SCRIPT, PHYSICS, AUDIO])
    }

    /// **Every family is on by default**: nothing is off until someone asks.
    #[test]
    fn every_family_is_on_by_default() {
        let activation = engine();

        for family in [SCRIPT, PHYSICS, AUDIO] {
            assert_eq!(activation.state(family), State::On, "{family}");
            assert!(activation.is_active(family), "{family}");
        }
        assert!(activation.inactive().is_empty());
    }

    /// **A request takes effect only at `settle`.** Switched off, the family
    /// is still on for the frame in force; settled, it is off. Switched on
    /// again, it stays off until the next settle.
    #[test]
    fn a_request_takes_effect_only_at_settle() {
        let activation = engine();

        activation.deactivate(SCRIPT).expect("a known family");
        assert_eq!(
            activation.state(SCRIPT),
            State::On,
            "requested, not in force"
        );
        assert!(activation.inactive().is_empty());

        assert!(activation.settle(), "the state in force changed");
        assert_eq!(activation.state(SCRIPT), State::Off);
        assert!(!activation.is_active(SCRIPT));
        assert_eq!(activation.inactive(), BTreeSet::from([SCRIPT]));
        assert!(activation.is_active(PHYSICS), "the others stay on");

        activation.activate(SCRIPT).expect("a known family");
        assert_eq!(activation.state(SCRIPT), State::Off, "until it settles");
        assert!(activation.settle());
        assert_eq!(activation.state(SCRIPT), State::On);
        assert!(activation.inactive().is_empty());
    }

    /// **The generation moves only when the state in force changes.** A
    /// settle with nothing requested, a request for the state already in
    /// force, or a request taken back before the boundary leave it where it
    /// was, and `settle` says nothing changed.
    #[test]
    fn the_generation_moves_only_when_the_state_changes() {
        let activation = engine();
        let start = activation.generation();

        assert!(!activation.settle(), "nothing requested");
        assert_eq!(activation.generation(), start);

        activation.activate(PHYSICS).expect("a known family");
        assert!(!activation.settle(), "already on");
        assert_eq!(activation.generation(), start);

        activation.deactivate(AUDIO).expect("a known family");
        activation.activate(AUDIO).expect("a known family");
        assert!(!activation.settle(), "taken back before the boundary");
        assert_eq!(activation.generation(), start);

        activation.deactivate(AUDIO).expect("a known family");
        assert!(activation.settle());
        let off = activation.generation();
        assert_ne!(off, start, "the state changed");

        assert!(!activation.settle(), "settled twice");
        assert_eq!(activation.generation(), off);
    }

    /// **A request naming an unknown family is refused**, and changes nothing:
    /// the family stays on, and the boundary finds nothing to settle.
    #[test]
    fn a_request_for_an_unknown_family_is_refused() {
        let activation = engine();
        let stranger = AgentId::new("stranger");
        let start = activation.generation();

        assert_eq!(
            activation.deactivate(stranger),
            Err(UnknownFamily(stranger))
        );
        assert_eq!(activation.activate(stranger), Err(UnknownFamily(stranger)));

        assert!(!activation.settle());
        assert_eq!(activation.generation(), start);
        assert_eq!(activation.state(stranger), State::On);
        assert!(activation.inactive().is_empty());
    }

    /// **The switch is shared**: threads requesting, reading and settling at
    /// once neither deadlock nor lose the last request.
    #[test]
    fn requests_reads_and_settles_from_several_threads_agree() {
        let activation = engine();
        let families = [SCRIPT, PHYSICS, AUDIO];

        std::thread::scope(|scope| {
            for (index, family) in families.into_iter().enumerate() {
                let activation = &activation;
                scope.spawn(move || {
                    for round in 0..200 {
                        if (round + index) % 2 == 0 {
                            activation.deactivate(family).expect("a known family");
                        } else {
                            activation.activate(family).expect("a known family");
                        }
                        let _ = activation.state(family);
                    }
                });
            }
            let activation = &activation;
            scope.spawn(move || {
                for _ in 0..200 {
                    let _ = activation.settle();
                    let _ = activation.inactive();
                    let _ = activation.generation();
                }
            });
        });

        for family in families {
            activation.deactivate(family).expect("a known family");
        }
        activation.settle();
        assert_eq!(activation.inactive(), BTreeSet::from(families));
    }
}
