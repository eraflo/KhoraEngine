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

//! The decision journal's switch: an environment variable naming a file has
//! the DCC record from the start, and the journal written there as JSON at
//! shutdown.

use std::ffi::OsString;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use khora_core::agent::gorna::DecisionTrace;

/// The environment variable naming the journal's file.
pub(crate) const RECORD_VAR: &str = "KHORA_RECORD_DECISIONS";

/// Returns the journal's file the variable's `value` names; `None` when unset
/// or empty.
pub(crate) fn record_path(value: Option<OsString>) -> Option<PathBuf> {
    value.filter(|value| !value.is_empty()).map(PathBuf::from)
}

/// Writes `trace` as JSON at `path`.
pub(crate) fn write(path: &Path, trace: &DecisionTrace) -> std::io::Result<()> {
    let mut file = BufWriter::new(File::create(path)?);
    serde_json::to_writer(&mut file, trace)?;
    file.flush()
}

/// Reads a journal written by [`write`].
#[cfg(test)]
pub(crate) fn read(path: &Path) -> std::io::Result<DecisionTrace> {
    let file = std::io::BufReader::new(File::open(path)?);
    Ok(serde_json::from_reader(file)?)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use khora_core::agent::gorna::protocol::{
        Allocation, CostBasis, DecidedBy, Envelope, Epoch, Offer, OfferRow, Report, Trigger, Why,
    };
    use khora_core::agent::gorna::{AgentId, Correction, EpochRecord, FrameSummary, StrategyId};
    use khora_core::agent::AgentImportance;
    use khora_core::lane::cost::{Cost, PerDevice};

    use super::*;

    /// **The record switch names the journal's file.** Unset or empty, it
    /// names none; set, its value is the file, relative or absolute.
    #[test]
    fn the_record_switch_names_the_journals_file() {
        assert_eq!(RECORD_VAR, "KHORA_RECORD_DECISIONS");
        assert_eq!(record_path(None), None, "unset: no journal");
        assert_eq!(
            record_path(Some(OsString::new())),
            None,
            "empty: no journal"
        );
        assert_eq!(
            record_path(Some(OsString::from("journal.json"))),
            Some(PathBuf::from("journal.json")),
            "a relative path"
        );
        let absolute = std::env::temp_dir().join("decisions").join("journal.json");
        assert_eq!(
            record_path(Some(absolute.clone().into_os_string())),
            Some(absolute),
            "an absolute path"
        );
    }

    /// `micros` on the host, nothing elsewhere.
    fn on_cpu(micros: u64) -> PerDevice<Duration> {
        PerDevice {
            cpu: Duration::from_micros(micros),
            ..PerDevice::default()
        }
    }

    /// A journal of two epochs naming two agents, holding what each round
    /// read and how the frames ran.
    fn journal() -> DecisionTrace {
        let oak = AgentId::new("journal-file-oak");
        let pine = AgentId::new("journal-file-pine");
        let epoch = |n: u64| EpochRecord {
            allocation: Allocation {
                epoch: Epoch(n),
                trigger: if n == 1 {
                    Trigger::Initial
                } else {
                    Trigger::FrameGovernor
                },
                envelopes: vec![
                    (
                        oak,
                        Envelope {
                            time: on_cpu(6_000),
                            ..Envelope::default()
                        },
                    ),
                    (
                        pine,
                        Envelope {
                            time: on_cpu(2_000),
                            ..Envelope::default()
                        },
                    ),
                ],
                pins: Vec::new(),
                allowed: Vec::new(),
                price: Some(0.5),
                lever: true,
                late: Vec::new(),
            },
            target: Duration::from_micros(16_667),
            offers: vec![Offer {
                agent: oak,
                epoch: Epoch(n),
                rows: vec![
                    OfferRow {
                        id: StrategyId::LowPower,
                        cost: Cost {
                            time: on_cpu(2_000),
                            ..Cost::default()
                        },
                        quality: 0.4,
                        confidence: CostBasis::Measured,
                    },
                    OfferRow {
                        id: StrategyId::HighPerformance,
                        cost: Cost {
                            time: on_cpu(5_000),
                            ..Cost::default()
                        },
                        quality: 1.0,
                        confidence: CostBasis::Quoted,
                    },
                ],
                floor: StrategyId::LowPower,
                current: StrategyId::LowPower,
                health: 1.0,
            }],
            reports: vec![Report {
                agent: oak,
                epoch: Epoch(n - 1),
                row: StrategyId::LowPower,
                measured: on_cpu(2_500),
                done: 1.0,
                health: 1.0,
                over_budget: false,
                stalled: false,
                workload: Some(40.0),
                switch: None,
                message: None,
            }],
            waves: vec![vec![oak, pine]],
            policies: Vec::new(),
            hints: Vec::new(),
            reasons: vec!["the frame ran over its target".to_owned()],
            why: Why {
                by: DecidedBy::Exact,
                trigger: Trigger::Initial,
                binding: None,
                rejected: Vec::new(),
            },
            importance: vec![
                (oak, AgentImportance::Critical),
                (pine, AgentImportance::Optional),
            ],
            corrections: vec![(
                oak,
                StrategyId::LowPower,
                Correction {
                    factor: 1.25,
                    spread: 0.125,
                },
            )],
            memory: PerDevice {
                gpu: Some(256 * 1024 * 1024),
                ..PerDevice::default()
            },
            background: vec![pine],
            at: Duration::from_millis(500 * n),
            frames: FrameSummary {
                count: 12,
                over_target: 1,
                worst: Duration::from_millis(21),
                mean: Duration::from_millis(14),
            },
            in_force: Vec::new(),
            planning: khora_core::agent::gorna::Planning::default(),
            planned: Vec::new(),
        };
        DecisionTrace {
            epochs: vec![epoch(1), epoch(2)],
        }
    }

    /// A file of the temporary directory no other test or run names.
    fn unique_file(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        std::env::temp_dir().join(format!("khora-{name}-{}-{nanos}.json", std::process::id()))
    }

    /// **A journal written then read is the same**, every epoch and what its
    /// round read. A file that cannot be written or read is an error, not a panic.
    #[test]
    fn a_journal_written_then_read_is_the_same() {
        let trace = journal();
        let path = unique_file("decision-journal");

        let written = write(&path, &trace);
        let read_back = read(&path);
        let unwritable = write(&path.join("journal.json"), &trace);
        let _ = std::fs::remove_file(&path);

        written.expect("the journal is written");
        let read_back = read_back.expect("the journal reads back");
        assert_eq!(read_back, trace, "the journal reads back as written");
        assert!(
            unwritable.is_err(),
            "a journal under a file cannot be written"
        );
        assert!(read(&path).is_err(), "a journal no longer there");
    }

    /// A headless game: no window, no world to speak of.
    struct Headless;

    impl crate::PhaseProvider for Headless {}
    impl crate::EngineApp for Headless {
        fn window_config() -> crate::WindowConfig {
            crate::WindowConfig::default()
        }
        fn new() -> Self {
            Headless
        }
        fn setup(&mut self, _world: &mut crate::GameWorld, _runtime: &khora_core::Runtime) {}
        fn update(&mut self, _world: &mut crate::GameWorld, _inputs: &[crate::InputEvent]) {}
    }

    /// **A journal the engine records replays, and reads its quotes' error.**
    ///
    /// A headless engine records for a few seconds, its frames overrunning then
    /// leaving room so the governor opens epochs. Each epoch, decided again,
    /// gives back the allocation recorded; the agents' runs, reported under the
    /// epochs they were lent, meet the quotes those epochs read.
    #[test]
    fn a_journal_the_engine_records_replays_and_reads_its_quotes_error() {
        use khora_control::evaluation::{replay, Arbiters};

        let mut engine = crate::EngineCore::<Headless>::new();
        engine.bootstrap(Headless, khora_core::Runtime::new());
        engine
            .dcc()
            .expect("the engine runs a DCC")
            .start_decision_recording();
        // Frames that overrun, then frames with room, as the scheduler
        // reports them: the governor moves the target, and epochs open.
        let frames = engine.dcc().expect("the engine runs a DCC").event_sender();
        let started = std::time::Instant::now();
        while started.elapsed() < Duration::from_secs(3) {
            engine.tick();
            let ms = if started.elapsed() < Duration::from_millis(1_500) {
                40
            } else {
                5
            };
            let _ = frames.send(khora_core::telemetry::TelemetryEvent::FrameReport(
                khora_core::agent::gorna::protocol::FrameReport {
                    busy: PerDevice {
                        cpu: Some(Duration::from_millis(ms)),
                        ..PerDevice::default()
                    },
                },
            ));
            std::thread::sleep(Duration::from_millis(5));
        }
        let trace = engine
            .dcc()
            .expect("the engine runs a DCC")
            .stop_decision_recording();
        engine.shutdown();

        assert!(!trace.epochs.is_empty(), "the DCC opened an epoch");
        let replayed = replay(&trace, Arbiters::Planned);
        for (again, recorded) in replayed.epochs.iter().zip(&trace.epochs) {
            assert_eq!(
                again.allocation, recorded.allocation,
                "epoch {:?} decided again",
                recorded.allocation.epoch
            );
        }
        let summary: Vec<_> = (trace.epochs.iter())
            .map(|epoch| {
                (
                    epoch.allocation.epoch,
                    epoch.allocation.trigger,
                    (epoch.reports.iter())
                        .map(|report| {
                            (
                                report.agent.name(),
                                report.epoch,
                                report.row,
                                report.measured.cpu,
                            )
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        assert!(
            replayed.metrics.prediction_error.is_some(),
            "{} epochs; no report met the quote of the epoch it ran in: {summary:#?}",
            trace.epochs.len()
        );
    }
}
