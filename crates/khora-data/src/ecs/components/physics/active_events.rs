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

use khora_macros::Component;
use serde::{Deserialize, Serialize};

/// Marks a collider whose contacts are reported as collision events.
///
/// Contacts are not reported by default, to save the cost. A sensor reports
/// what enters and leaves it without this marker.
// `Authored`, hence saved: a designer adds it to say "report this collider's contacts".
#[derive(Debug, Clone, Copy, Default, Component, Serialize, Deserialize)]
#[component(domain = Physics)]
pub struct ActiveEvents;
