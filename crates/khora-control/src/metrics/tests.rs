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

use super::*;

#[test]
fn test_metric_store_variance_and_extremes() {
    let mut store = MetricStore::new();
    let id = MetricId::new("test", "values");
    store.push(id.clone(), 5.0);
    store.push(id.clone(), 15.0);
    store.push(id.clone(), 5.0);
    store.push(id.clone(), 15.0);

    assert!((store.get_variance(&id) - 25.0).abs() < 0.01);
    assert_eq!(store.get_min(&id), 5.0);
    assert_eq!(store.get_max(&id), 15.0);
    assert_eq!(store.get_sample_count(&id), 4);
}
