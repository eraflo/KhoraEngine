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

//! Scene module containing the Scene struct and related functionality.

pub mod component_registration;
pub mod material_registration;
mod recipe;
pub mod shape;

pub mod migrations;
pub mod record;
mod strategy;

pub use component_registration::*;
pub use material_registration::*;
pub use recipe::*;
pub use shape::{ComponentShape, FieldSchema};

pub use migrations::*;
pub use strategy::*;
