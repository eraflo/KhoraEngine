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

//! A dual-output logger that captures log entries for the editor console
//! while also printing to stderr for development.

use super::state::{LogEntry, LogLevel};
use std::sync::{Arc, Mutex};

/// Maximum number of log entries kept in memory.
const MAX_LOG_ENTRIES: usize = 2048;

/// A `log::Log` implementation that stores entries in a shared buffer
/// and also writes to stderr.
pub struct EditorLogCapture {
    entries: Arc<Mutex<Vec<LogEntry>>>,
}

impl EditorLogCapture {
    /// Creates a new capture logger.
    ///
    /// Returns the logger and a handle to the shared entry buffer.
    /// The handle should be used by the editor state to read log entries.
    pub fn new() -> (Self, Arc<Mutex<Vec<LogEntry>>>) {
        let entries = Arc::new(Mutex::new(Vec::new()));
        let handle = Arc::clone(&entries);
        (Self { entries }, handle)
    }
}

/// Targets whose DEBUG/TRACE output is suppressed to avoid log flooding.
const NOISY_TARGETS: &[&str] = &["naga", "wgpu_core", "wgpu_hal"];

impl log::Log for EditorLogCapture {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        // Suppress debug/trace from noisy GPU-related crates.
        if metadata.level() > log::Level::Info {
            let target = metadata.target();
            for prefix in NOISY_TARGETS {
                if target.starts_with(prefix) {
                    return false;
                }
            }
        }
        metadata.level() <= log::Level::Debug
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        // Print to stderr for development debugging.
        eprintln!(
            "[{}] {}: {}",
            record.level(),
            record.target(),
            record.args()
        );

        let entry = LogEntry {
            level: match record.level() {
                log::Level::Error => LogLevel::Error,
                log::Level::Warn => LogLevel::Warn,
                log::Level::Info => LogLevel::Info,
                log::Level::Debug => LogLevel::Debug,
                log::Level::Trace => LogLevel::Trace,
            },
            message: record.args().to_string(),
            target: record.target().to_string(),
        };

        if let Ok(mut entries) = self.entries.lock() {
            if entries.len() >= MAX_LOG_ENTRIES {
                // Drop oldest quarter when full.
                let drain_count = MAX_LOG_ENTRIES / 4;
                entries.drain(..drain_count);
            }
            entries.push(entry);
        }
    }

    fn flush(&self) {}
}
