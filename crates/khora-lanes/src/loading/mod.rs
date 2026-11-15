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

//! The `loading` family: scenes, scripts and assets loaded off the frame, each
//! result staged under its lease for a frame's boundary.
//!
//! [`RushLane`] loads everything filed; [`PacedLane`] only what its budget covers.

mod family;
mod inputs;
mod paced;
mod registration;
mod rush;
mod work;

pub use family::*;
pub use inputs::*;
pub use paced::*;
pub use rush::*;
