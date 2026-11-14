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

//! The editor's composite widgets, used by its panels: free functions over a
//! `&mut dyn UiBuilder` that paint at absolute screen coordinates.

pub mod asset_kind;
pub mod brand;
pub mod enum_variants;
pub mod inspector;
pub mod panel_header;
pub mod tile;
