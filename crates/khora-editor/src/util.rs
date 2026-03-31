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

//! Utility helpers for the editor application.

use khora_core::ui::editor::*;
use khora_sdk::prelude::ecs::Transform;
use khora_sdk::prelude::math::{Quaternion, Vec3};

/// Serialize a Transform into 40 bytes (translation: 12, rotation: 16, scale: 12).
pub fn bytemuck_transform(t: &Transform) -> [u8; 40] {
    let mut buf = [0u8; 40];
    buf[0..4].copy_from_slice(&t.translation.x.to_le_bytes());
    buf[4..8].copy_from_slice(&t.translation.y.to_le_bytes());
    buf[8..12].copy_from_slice(&t.translation.z.to_le_bytes());
    buf[12..16].copy_from_slice(&t.rotation.x.to_le_bytes());
    buf[16..20].copy_from_slice(&t.rotation.y.to_le_bytes());
    buf[20..24].copy_from_slice(&t.rotation.z.to_le_bytes());
    buf[24..28].copy_from_slice(&t.rotation.w.to_le_bytes());
    buf[28..32].copy_from_slice(&t.scale.x.to_le_bytes());
    buf[32..36].copy_from_slice(&t.scale.y.to_le_bytes());
    buf[36..40].copy_from_slice(&t.scale.z.to_le_bytes());
    buf
}

/// Deserialize a Transform from 40 bytes.
pub fn unbytemuck_transform(data: &[u8]) -> Transform {
    let tx = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let ty = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let tz = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let rx = f32::from_le_bytes([data[12], data[13], data[14], data[15]]);
    let ry = f32::from_le_bytes([data[16], data[17], data[18], data[19]]);
    let rz = f32::from_le_bytes([data[20], data[21], data[22], data[23]]);
    let rw = f32::from_le_bytes([data[24], data[25], data[26], data[27]]);
    let sx = f32::from_le_bytes([data[28], data[29], data[30], data[31]]);
    let sy = f32::from_le_bytes([data[32], data[33], data[34], data[35]]);
    let sz = f32::from_le_bytes([data[36], data[37], data[38], data[39]]);
    Transform {
        translation: Vec3::new(tx, ty, tz),
        rotation: Quaternion::new(rx, ry, rz, rw),
        scale: Vec3::new(sx, sy, sz),
    }
}
