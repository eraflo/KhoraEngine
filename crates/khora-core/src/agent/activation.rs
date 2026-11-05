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

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use super::gorna::AgentId;

/// What a family does, for the frame in force.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum State {
    /// The family runs: its agent, its systems and its Flows.
    On,
    /// The family does not run.
    Off,
    /// The family runs, on the entities holding the marker component
    /// registered under this name only.
    Scoped(&'static str),
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
    /// Every family not plainly on, as requested; a family absent is on.
    requested: BTreeMap<AgentId, State>,
    /// The same, in force.
    in_force: BTreeMap<AgentId, State>,
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
        self.request(family, State::On)
    }

    /// Requests `family` off, from the next [`settle`](Self::settle).
    pub fn deactivate(&self, family: AgentId) -> Result<(), UnknownFamily> {
        self.request(family, State::Off)
    }

    /// Requests `family` on, limited to the entities holding the component
    /// registered as `marker`, from the next [`settle`](Self::settle). A
    /// scoped family counts as on.
    ///
    /// The scope is honoured by the family's Flows and the systems that read
    /// the World for it — the scripting and audio families today. The physics
    /// family does not honour one yet: its provider steps every body it was
    /// given, so a scoped physics family runs as one plainly on.
    pub fn scope(&self, family: AgentId, marker: &'static str) -> Result<(), UnknownFamily> {
        self.request(family, State::Scoped(marker))
    }

    /// The state in force of `family`. A family the engine does not know is
    /// never switched, so it is on.
    pub fn state(&self, family: AgentId) -> State {
        self.read()
            .in_force
            .get(&family)
            .copied()
            .unwrap_or(State::On)
    }

    /// Whether `family` runs, in the state in force: on, or scoped.
    pub fn is_active(&self, family: AgentId) -> bool {
        self.state(family) != State::Off
    }

    /// The families off, in the state in force. A scoped family is not one.
    pub fn inactive(&self) -> BTreeSet<AgentId> {
        self.read()
            .in_force
            .iter()
            .filter(|(_, state)| **state == State::Off)
            .map(|(family, _)| *family)
            .collect()
    }

    /// The families scoped in the state in force, each with its marker.
    pub fn scopes(&self) -> Vec<(AgentId, &'static str)> {
        self.read()
            .in_force
            .iter()
            .filter_map(|(family, state)| match state {
                State::Scoped(marker) => Some((*family, *marker)),
                _ => None,
            })
            .collect()
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
        if switches.requested == switches.in_force {
            return false;
        }
        switches.in_force = switches.requested.clone();
        switches.generation += 1;
        true
    }

    fn request(&self, family: AgentId, state: State) -> Result<(), UnknownFamily> {
        let mut switches = self.write();
        if !switches.known.contains(&family) {
            log::warn!("a request to switch `{family}`, which is no registered family, is refused");
            return Err(UnknownFamily(family));
        }
        match state {
            State::On => switches.requested.remove(&family),
            other => switches.requested.insert(family, other),
        };
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

    /// **A scoped family is on, limited to its marker.** Requested, the scope
    /// waits for the boundary like any request; settled, the family reports
    /// its marker, counts as active and is not among the families off.
    #[test]
    fn a_scoped_family_is_on_and_reports_its_marker() {
        let activation = engine();

        assert_eq!(activation.scope(SCRIPT, "Preview"), Ok(()));
        assert_eq!(
            activation.state(SCRIPT),
            State::On,
            "requested, not in force"
        );

        assert!(activation.settle(), "the state in force changed");
        assert_eq!(activation.state(SCRIPT), State::Scoped("Preview"));
        assert!(activation.is_active(SCRIPT), "a scoped family is on");
        assert!(
            activation.inactive().is_empty(),
            "a scoped family is not off: {:?}",
            activation.inactive()
        );
        assert_eq!(activation.state(AUDIO), State::On, "the others unscoped");
    }

    /// **`activate` and `deactivate` replace a scope**, and a scope replaces
    /// a family switched off: the last request before the boundary is the
    /// state in force.
    #[test]
    fn activate_and_deactivate_replace_a_scope() {
        let activation = engine();

        activation.scope(AUDIO, "Preview").expect("a known family");
        activation.settle();
        activation.deactivate(AUDIO).expect("a known family");
        assert!(activation.settle());
        assert_eq!(
            activation.state(AUDIO),
            State::Off,
            "off replaces the scope"
        );
        assert_eq!(activation.inactive(), BTreeSet::from([AUDIO]));

        activation.scope(AUDIO, "Preview").expect("a known family");
        assert!(activation.settle());
        assert_eq!(
            activation.state(AUDIO),
            State::Scoped("Preview"),
            "a scope replaces off"
        );
        assert!(activation.inactive().is_empty());

        activation.activate(AUDIO).expect("a known family");
        assert!(activation.settle());
        assert_eq!(activation.state(AUDIO), State::On, "on replaces the scope");
    }

    /// **The generation moves when the marker changes, and only then.** The
    /// same scope requested again, or a scope taken back before the boundary,
    /// changes nothing; another marker, or leaving the scope, does.
    #[test]
    fn the_generation_moves_when_the_scope_changes_and_not_for_the_same_scope() {
        let activation = engine();
        let start = activation.generation();

        activation.scope(SCRIPT, "Preview").expect("a known family");
        activation.activate(SCRIPT).expect("a known family");
        assert!(!activation.settle(), "taken back before the boundary");
        assert_eq!(activation.generation(), start);

        activation.scope(SCRIPT, "Preview").expect("a known family");
        assert!(activation.settle(), "on to scoped");
        let scoped = activation.generation();
        assert_ne!(scoped, start);

        activation.scope(SCRIPT, "Preview").expect("a known family");
        assert!(!activation.settle(), "the same scope again");
        assert_eq!(activation.generation(), scoped);

        activation
            .scope(SCRIPT, "Inspected")
            .expect("a known family");
        assert!(activation.settle(), "another marker");
        let moved = activation.generation();
        assert_ne!(moved, scoped);
        assert_eq!(activation.state(SCRIPT), State::Scoped("Inspected"));

        activation.activate(SCRIPT).expect("a known family");
        assert!(activation.settle(), "scoped to on");
        assert_ne!(activation.generation(), moved);
    }

    /// **A scope naming an unknown family is refused**, and changes nothing.
    #[test]
    fn a_scope_for_an_unknown_family_is_refused() {
        let activation = engine();
        let stranger = AgentId::new("stranger");
        let start = activation.generation();

        assert_eq!(
            activation.scope(stranger, "Preview"),
            Err(UnknownFamily(stranger))
        );

        assert!(!activation.settle());
        assert_eq!(activation.generation(), start);
        assert_eq!(activation.state(stranger), State::On);
    }

    /// **The scopes in force, each with its marker**: a scope requested is
    /// not listed before it settles, and one replaced is not listed after.
    #[test]
    fn the_scopes_in_force_are_listed_with_their_markers() {
        let activation = engine();
        activation.scope(AUDIO, "Preview").expect("a known family");
        assert!(activation.scopes().is_empty(), "requested, not in force");

        activation.settle();
        assert_eq!(activation.scopes(), vec![(AUDIO, "Preview")]);

        activation.activate(AUDIO).expect("a known family");
        activation.settle();
        assert!(activation.scopes().is_empty(), "replaced by on");
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
