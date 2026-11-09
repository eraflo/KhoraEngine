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

//! What the background tests share: a family that bakes off the frame, the
//! orders it takes from its mailbox, the loaves it stages, and lanes that read
//! one source each.

use std::sync::{Arc, Mutex};

use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::asset::Asset;
use khora_core::event::{BetweenSessions, Mailbox, Reader, Supersedes};
use khora_core::lane::cost::Cost;
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::io::{Budget, Locked, Target};
use khora_core::lane::level::Level;
use khora_core::lane::Staged;
use khora_core::lane::{Lane, LaneReport};
use khora_core::lane::{LaneError, LaneKind};
use khora_core::residency::{DeviceClass, Residency, ResidencyError, Resident};
use khora_macros::{LaneInputs, LaneOutputs};

use crate::fixtures::{level, Contacts, Counter, MixSettings, Positions};

/// The bakery's agent: the family a `Take` mailbox of [`Order`] names.
pub const BAKE: AgentId = AgentId::new("bake");

/// Another family, taking [`Theirs`].
pub const NOT_BAKE: AgentId = AgentId::new("not_bake");

// ─── Requests ───────────────────────────────────────────────────────────────

/// A request the bakery takes from its mailbox.
#[derive(Debug, Clone, PartialEq)]
pub struct Order(pub u32);
impl Supersedes for Order {}

/// A request no mailbox declares.
#[derive(Debug, Clone, PartialEq)]
pub struct Unclaimed(pub u32);
impl Supersedes for Unclaimed {}

/// A request the bakery reads by cursor, not takes.
#[derive(Debug, Clone, PartialEq)]
pub struct Watched(pub u32);
impl Supersedes for Watched {}

/// A request another family takes.
#[derive(Debug, Clone, PartialEq)]
pub struct Theirs(pub u32);
impl Supersedes for Theirs {}

inventory::submit! {
    Mailbox::of::<Order>(BAKE, Reader::Take, BetweenSessions::Hold)
}
inventory::submit! {
    Mailbox::of::<Watched>(BAKE, Reader::Cursor("bake"), BetweenSessions::Drop)
}
inventory::submit! {
    Mailbox::of::<Theirs>(NOT_BAKE, Reader::Take, BetweenSessions::Drop)
}

// ─── Staged outputs ─────────────────────────────────────────────────────────

/// What abandoned loaves wrote: the key and batch of each, in order.
pub type Abandoned = Arc<Mutex<Vec<(u32, u32)>>>;

/// A staged output: the loaf of one key, from one batch. A newer batch of the
/// same key supersedes it; abandoned, it writes itself into its log.
#[derive(Debug, Clone)]
pub struct Loaf {
    pub key: u32,
    pub batch: u32,
    pub abandoned: Abandoned,
}

impl Loaf {
    pub fn new(key: u32, batch: u32, abandoned: &Abandoned) -> Self {
        Self {
            key,
            batch,
            abandoned: Arc::clone(abandoned),
        }
    }
}

/// Two loaves are the same when their key and batch are.
impl PartialEq for Loaf {
    fn eq(&self, other: &Self) -> bool {
        (self.key, self.batch) == (other.key, other.batch)
    }
}

impl Supersedes for Loaf {
    const COALESCES: bool = true;

    fn supersedes(&self, earlier: &Self) -> bool {
        self.key == earlier.key
    }
}

impl Staged for Loaf {
    fn abandon(self) {
        self.abandoned
            .lock()
            .expect("the abandon log")
            .push((self.key, self.batch));
    }
}

/// A staged output that never supersedes another.
#[derive(Debug, Clone, PartialEq)]
pub struct Note(pub u32);
impl Supersedes for Note {}
impl Staged for Note {}

/// The key and batch of each loaf, in order.
pub fn batches(loaves: &[Loaf]) -> Vec<(u32, u32)> {
    loaves.iter().map(|loaf| (loaf.key, loaf.batch)).collect()
}

/// What `abandoned` holds.
pub fn abandoned(abandoned: &Abandoned) -> Vec<(u32, u32)> {
    abandoned.lock().expect("the abandon log").clone()
}

// ─── A residency ────────────────────────────────────────────────────────────

/// A CPU asset a glaze is made from.
pub struct Recipe;
impl Asset for Recipe {}

/// A residency of glazes, for a lane reading `#[resident]`.
pub struct Glazes;

impl Residency for Glazes {
    const NAME: &'static str = "glazes";
    const DEVICE: DeviceClass = DeviceClass::Cpu;
    type Source = Recipe;
    type Resident = u32;
    type Target<'a> = ();

    fn make(_source: &Recipe, _target: &mut ()) -> Result<u32, ResidencyError> {
        Ok(0)
    }

    fn release(_resident: u32, _target: &mut ()) {}
}

// ─── Families ───────────────────────────────────────────────────────────────

/// The outputs of a family that writes nothing: what its lanes read is all a
/// test looks at.
#[derive(LaneOutputs)]
pub struct Nothing {}

/// A family whose lanes each read one source.
pub struct QuietFamily;

impl LaneFamily for QuietFamily {
    const KIND: LaneKind = LaneKind::Ui;
    const NAME: &'static str = "quiet";
    const AGENT: AgentId = AgentId::new("quiet");
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = Nothing;
}

/// The bakery's outputs: the loaves it staged.
#[derive(LaneOutputs)]
pub struct BakeOutputs {
    #[staged]
    pub loaves: Vec<Loaf>,
}

/// The bakery, as boot sees it: it takes orders. Its lanes here write
/// nothing, so boot reads only their inputs.
pub struct BakeFamily;

impl LaneFamily for BakeFamily {
    const KIND: LaneKind = LaneKind::Audio;
    const NAME: &'static str = "bake";
    const AGENT: AgentId = BAKE;
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = Nothing;
}

// ─── Inputs ─────────────────────────────────────────────────────────────────

/// What outlives a frame: a runtime entry and the budget.
#[derive(LaneInputs)]
#[allow(dead_code)]
pub struct OffFrame<'f> {
    #[resource]
    pub settings: &'f MixSettings,
    #[budget]
    pub budget: Option<Budget>,
}

/// The orders the bakery's mailbox holds, a runtime entry and the budget.
#[derive(LaneInputs)]
#[allow(dead_code)]
pub struct TakesOrders<'f> {
    #[take]
    pub orders: Vec<Order>,
    #[resource]
    pub settings: &'f MixSettings,
    #[budget]
    pub budget: Option<Budget>,
}

/// The orders alone.
#[derive(LaneInputs)]
pub struct OnlyOrders {
    #[take]
    pub orders: Vec<Order>,
}

/// A View.
#[derive(LaneInputs)]
#[allow(dead_code)]
pub struct ReadsBus<'f> {
    #[bus]
    pub view: &'f Positions,
}

/// The frame's target.
#[derive(LaneInputs)]
#[allow(dead_code)]
pub struct ReadsFrame {
    #[frame]
    pub target: Target,
}

/// A slot an earlier phase wrote.
#[derive(LaneInputs)]
#[allow(dead_code)]
pub struct ReadsDeck<'f> {
    #[deck]
    pub earlier: Option<&'f Contacts>,
}

/// What a residency made usable.
#[derive(LaneInputs)]
#[allow(dead_code)]
pub struct ReadsResident<'f> {
    #[resident]
    pub glazes: &'f Resident<Glazes>,
}

/// A lock.
#[derive(LaneInputs)]
#[allow(dead_code)]
pub struct ReadsLock<'f> {
    #[locked]
    pub counter: Locked<'f, Counter>,
}

/// Requests no mailbox declares.
#[derive(LaneInputs)]
#[allow(dead_code)]
pub struct TakesUnclaimed {
    #[take]
    pub unclaimed: Vec<Unclaimed>,
}

/// Requests the bakery reads by cursor.
#[derive(LaneInputs)]
#[allow(dead_code)]
pub struct TakesWatched {
    #[take]
    pub watched: Vec<Watched>,
}

/// Requests another family takes.
#[derive(LaneInputs)]
#[allow(dead_code)]
pub struct TakesTheirs {
    #[take]
    pub theirs: Vec<Theirs>,
}

// ─── Lanes ──────────────────────────────────────────────────────────────────

const LEVELS: &[Level] = &[level(StrategyId::Balanced, "bake")];

/// A lane of `$family` reading `$inputs` — borrowed for the frame (`'f`) or
/// owned — writing nothing.
macro_rules! reading_lane {
    ($lane:ident, $family:ty, $inputs:ident<'f>) => {
        reading_lane!(@lane $lane, $family, $inputs<'f>, $inputs<'_>);
    };
    ($lane:ident, $family:ty, $inputs:ident) => {
        reading_lane!(@lane $lane, $family, $inputs, $inputs);
    };
    (@lane $lane:ident, $family:ty, $frame:ty, $any:ty) => {
        #[derive(Default)]
        pub struct $lane;

        impl Lane for $lane {
            type Family = $family;
            type Inputs<'f> = $frame;

            fn levels(&self) -> &[Level] {
                LEVELS
            }

            fn cost(&self, _level: usize, _inputs: &$any) -> Cost {
                Cost::default()
            }

            fn record(
                &mut self,
                _level: usize,
                _inputs: &mut $any,
                _out: &mut Nothing,
            ) -> Result<LaneReport, LaneError> {
                Ok(LaneReport::default())
            }
        }
    };
}

reading_lane!(OffFrameLane, QuietFamily, OffFrame<'f>);
reading_lane!(BusLane, QuietFamily, ReadsBus<'f>);
reading_lane!(FrameLane, QuietFamily, ReadsFrame);
reading_lane!(DeckLane, QuietFamily, ReadsDeck<'f>);
reading_lane!(ResidentLane, QuietFamily, ReadsResident<'f>);
reading_lane!(LockLane, QuietFamily, ReadsLock<'f>);
reading_lane!(OrdersLane, BakeFamily, TakesOrders<'f>);
reading_lane!(UnclaimedLane, BakeFamily, TakesUnclaimed);
reading_lane!(WatchedLane, BakeFamily, TakesWatched);
reading_lane!(TheirsLane, BakeFamily, TakesTheirs);
