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

//! Reads the machine the engine runs on: its CPU and operating system,
//! through `sysinfo`.

use khora_core::platform::MachineIdentity;
use khora_core::renderer::api::device::AdapterIdentity;

/// The machine the engine runs on: `gpu`, the adapter the graphics device
/// reported (`None` without one), with the CPU's brand and core count and
/// the operating system's long version, read through `sysinfo`.
pub fn read_machine(gpu: Option<AdapterIdentity>) -> MachineIdentity {
    let mut system = sysinfo::System::new();
    system.refresh_cpu_all();
    let cpu = (system.cpus().first())
        .map(|cpu| cpu.brand().trim().to_owned())
        .filter(|brand| !brand.is_empty())
        .unwrap_or_else(|| std::env::consts::ARCH.to_owned());
    let cores = sysinfo::System::physical_core_count()
        .filter(|cores| *cores > 0)
        .unwrap_or_else(|| system.cpus().len().max(1));
    // The long name gives the edition (`Windows 11 Home`); the version the
    // build (`11 (26200)`): an update changes what a frame costs, an edition
    // does not, so both are named.
    let name = sysinfo::System::long_os_version()
        .filter(|os| !os.trim().is_empty())
        .unwrap_or_else(|| std::env::consts::OS.to_owned());
    let os = match sysinfo::System::os_version().filter(|version| !version.trim().is_empty()) {
        Some(version) if !name.contains(&version) => format!("{name} {version}"),
        _ => name,
    };
    MachineIdentity {
        gpu,
        cpu,
        cores: u32::try_from(cores).unwrap_or(u32::MAX),
        os,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The machine identity reads the CPU and the OS.** Read on the machine
    /// running the test, the CPU and the OS are named and the CPU has cores;
    /// without a graphics device there is no GPU, and the adapter a device
    /// reported is kept as it was given.
    #[test]
    fn the_machine_identity_reads_the_cpu_and_os() {
        let without_gpu = read_machine(None);
        assert!(!without_gpu.cpu.trim().is_empty(), "the CPU is named");
        assert!(!without_gpu.os.trim().is_empty(), "the OS is named");
        assert!(without_gpu.cores > 0, "the CPU has cores");
        assert_eq!(without_gpu.gpu, None, "no device, no GPU");

        let adapter = AdapterIdentity {
            name: "Example GPU 9000".to_owned(),
            vendor: 0x10de,
            device: 0x2786,
            driver: "Example Driver".to_owned(),
            driver_info: "566.36".to_owned(),
        };
        let with_gpu = read_machine(Some(adapter.clone()));
        assert_eq!(with_gpu.gpu, Some(adapter), "the device's adapter is kept");
        assert_eq!(
            (&with_gpu.cpu, with_gpu.cores, &with_gpu.os),
            (&without_gpu.cpu, without_gpu.cores, &without_gpu.os),
            "the same machine reads the same CPU and OS"
        );
    }

    /// **The machine identity names the OS's version, not only its name.**
    /// An OS update can change what a frame costs, so the machine's `os`
    /// carries the version the platform reports (`sysinfo`'s `os_version`,
    /// e.g. `11 (26200)` on Windows, `22.04` on Ubuntu, `14.0` on macOS) —
    /// two builds of one OS are two machines.
    #[test]
    fn the_machine_identity_names_the_os_version() {
        let machine = read_machine(None);
        if let Some(version) = sysinfo::System::os_version().filter(|v| !v.trim().is_empty()) {
            assert!(
                machine.os.contains(version.trim()),
                "the OS {:?} does not name its version {:?}",
                machine.os,
                version
            );
        }
    }
}
