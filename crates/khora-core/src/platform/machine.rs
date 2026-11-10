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

//! The machine the engine runs on, named by what decides what a frame costs.

use serde::{Deserialize, Serialize};

use crate::renderer::api::device::AdapterIdentity;

/// The machine the engine runs on, as far as what it costs to run a frame
/// goes: its GPU adapter, its CPU and its operating system.
///
/// Filled by the platform backend at boot and put in the runtime's
/// resources. What the engine learns of this machine is filed under its
/// [`key`](Self::key).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineIdentity {
    /// The GPU adapter the renderer runs on; `None` with no graphics device.
    pub gpu: Option<AdapterIdentity>,
    /// The CPU's model, as the platform names it.
    pub cpu: String,
    /// How many cores the CPU has.
    pub cores: u32,
    /// The operating system's name and version.
    pub os: String,
}

impl MachineIdentity {
    /// A short, stable key: 16 lowercase hex digits of a hash of every field,
    /// the same on every run and every build.
    ///
    /// The hash is FNV-1a 64 (offset basis `0xcbf29ce484222325`, prime
    /// `0x100000001b3`), never std's `DefaultHasher`, over this canonical
    /// encoding, in order:
    ///
    /// - the GPU: one byte `0` for `None`; for `Some`, one byte `1`, then
    ///   `name`, `vendor`, `device`, `driver`, `driver_info`;
    /// - then `cpu`, `cores`, `os`.
    ///
    /// A string is its UTF-8 byte length as a little-endian `u32`, then its
    /// bytes; a `u32` is its four little-endian bytes. The length prefix keeps
    /// two fields from trading characters without the key changing. The key is
    /// the hash written as `{:016x}`.
    pub fn key(&self) -> String {
        let mut hash = Fnv::default();
        match &self.gpu {
            None => hash.byte(0),
            Some(gpu) => {
                hash.byte(1);
                hash.text(&gpu.name);
                hash.number(gpu.vendor);
                hash.number(gpu.device);
                hash.text(&gpu.driver);
                hash.text(&gpu.driver_info);
            }
        }
        hash.text(&self.cpu);
        hash.number(self.cores);
        hash.text(&self.os);
        format!("{:016x}", hash.0)
    }
}

/// FNV-1a 64, fed the canonical encoding [`MachineIdentity::key`] names.
struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    fn byte(&mut self, byte: u8) {
        self.0 ^= u64::from(byte);
        self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.byte(byte);
        }
    }

    fn number(&mut self, value: u32) {
        self.bytes(&value.to_le_bytes());
    }

    fn text(&mut self, text: &str) {
        self.number(u32::try_from(text.len()).unwrap_or(u32::MAX));
        self.bytes(text.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A machine with every field set.
    fn a_machine() -> MachineIdentity {
        MachineIdentity {
            gpu: Some(AdapterIdentity {
                name: "Example GPU 9000".to_owned(),
                vendor: 0x10de,
                device: 0x2786,
                driver: "Example Driver".to_owned(),
                driver_info: "566.36".to_owned(),
            }),
            cpu: "Example CPU 8-Core Processor".to_owned(),
            cores: 16,
            os: "Example OS 11".to_owned(),
        }
    }

    /// **A machine's key is stable and changes with what it is made of.** The
    /// same fields give the same key, on every run and every build: the key of
    /// a fixed machine is pinned here, so the hash or its encoding can never
    /// drift without this test reddening (every profile already written would
    /// be orphaned). It is 16 lowercase hex digits. Another driver, another
    /// driver version, another device, another CPU, another core count,
    /// another OS or no GPU at all give another key, and so do two fields
    /// trading characters.
    #[test]
    fn a_machine_key_is_stable_and_changes_with_the_driver() {
        let machine = a_machine();
        let key = machine.key();

        assert_eq!(key, "05ec48b3a8f88de3", "the pinned key of a fixed machine");
        assert_eq!(
            MachineIdentity::default().key(),
            "7c96179f62dae92f",
            "the pinned key of a machine with no GPU and empty fields"
        );
        assert_eq!(machine.clone().key(), key, "the same fields, the same key");
        assert_eq!(key.len(), 16, "{key}");
        assert!(
            key.chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
            "16 lowercase hex digits: {key}"
        );

        let with_gpu = |change: fn(&mut AdapterIdentity)| {
            let mut other = a_machine();
            if let Some(gpu) = other.gpu.as_mut() {
                change(gpu);
            }
            other.key()
        };
        let others = [
            (
                "another driver",
                with_gpu(|gpu| gpu.driver = "Other Driver".to_owned()),
            ),
            (
                "another driver version",
                with_gpu(|gpu| gpu.driver_info = "572.16".to_owned()),
            ),
            ("another vendor", with_gpu(|gpu| gpu.vendor = 0x1002)),
            ("another device", with_gpu(|gpu| gpu.device = 0x2704)),
            (
                "another adapter name",
                with_gpu(|gpu| gpu.name = "Example GPU 9001".to_owned()),
            ),
            (
                "two fields trading a character",
                with_gpu(|gpu| {
                    gpu.driver = "Example Drive".to_owned();
                    gpu.driver_info = "r566.36".to_owned();
                }),
            ),
            (
                "another CPU",
                MachineIdentity {
                    cpu: "Other CPU 6-Core Processor".to_owned(),
                    ..a_machine()
                }
                .key(),
            ),
            (
                "another core count",
                MachineIdentity {
                    cores: 8,
                    ..a_machine()
                }
                .key(),
            ),
            (
                "another OS",
                MachineIdentity {
                    os: "Example OS 12".to_owned(),
                    ..a_machine()
                }
                .key(),
            ),
            (
                "no GPU",
                MachineIdentity {
                    gpu: None,
                    ..a_machine()
                }
                .key(),
            ),
        ];
        for (what, other) in &others {
            assert_ne!(other, &key, "{what} gives the same key");
        }
    }

    /// **A machine identity round-trips through JSON.** Written beside what
    /// was learnt of the machine, it reads back equal, its GPU included — and
    /// a machine without a GPU reads back without one.
    #[test]
    fn a_machine_identity_round_trips() {
        for machine in [
            a_machine(),
            MachineIdentity {
                gpu: None,
                ..a_machine()
            },
        ] {
            let text = serde_json::to_string(&machine).expect("a machine identity writes");
            let read: MachineIdentity =
                serde_json::from_str(&text).expect("a machine identity reads");
            assert_eq!(read, machine);
        }
    }
}
