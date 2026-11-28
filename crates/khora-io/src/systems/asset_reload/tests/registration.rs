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

//! Where the reload sits in the tick: before the resolver.

use super::*;

/// The refreshed copies must be in place before the resolver reads the
/// store's arrivals, whatever order hints other systems carry.
#[test]
fn the_resolver_runs_after_asset_reload() {
    let reload = data_system("asset_reload");
    let resolver = data_system("asset_resolver");
    assert_eq!(reload.phase, resolver.phase, "both run in one phase");
    assert!(
        resolver.runs_after.contains(&"asset_reload"),
        "the resolver declares it runs after asset_reload: {:?}",
        resolver.runs_after
    );
}
