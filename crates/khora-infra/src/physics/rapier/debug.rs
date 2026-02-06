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

use khora_core::math::Vec3;
use rapier3d::prelude::*;

#[derive(Default)]
pub struct RapierDebugBackend {
    pub vertices: Vec<Vec3>,
    pub indices: Vec<[u32; 2]>,
}

impl DebugRenderBackend for RapierDebugBackend {
    fn draw_line(
        &mut self,
        _: DebugRenderObject,
        a: Point<Real>,
        b: Point<Real>,
        _color: [f32; 4],
    ) {
        let start = Vec3::new(a.x, a.y, a.z);
        let end = Vec3::new(b.x, b.y, b.z);
        let base_idx = self.vertices.len() as u32;
        self.vertices.push(start);
        self.vertices.push(end);
        self.indices.push([base_idx, base_idx + 1]);
    }
}
