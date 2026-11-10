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

//! Compares GORNA's proposer sets: on the simulated scenarios, and on a
//! journal the engine recorded when one is given.
//!
//! ```text
//! cargo run --release -p khora-sdk --example arbiter_eval [journal.json]
//! ```
//!
//! A journal is recorded by running any app with
//! `KHORA_RECORD_DECISIONS=<file>`: the engine writes it at shutdown.
//!
//! The tables are the program's output, written to the standard output;
//! decision times are only meaningful in a release build.

use std::fs::File;
use std::io::{self, BufReader, Write};
use std::time::Duration;

use khora_control::evaluation::{replay, Arbiters, Scenario};
use khora_core::agent::gorna::DecisionTrace;

/// The seed the scenarios draw their noise from.
const SEED: u64 = 1;

fn main() -> anyhow::Result<()> {
    let mut out = io::stdout().lock();
    scenarios(&mut out)?;
    if let Some(path) = std::env::args_os().nth(1) {
        let trace: DecisionTrace = serde_json::from_reader(BufReader::new(File::open(&path)?))?;
        writeln!(out)?;
        journal(&mut out, &path.to_string_lossy(), &trace)?;
    }
    Ok(())
}

/// Every scenario run by every proposer set.
fn scenarios(out: &mut impl Write) -> io::Result<()> {
    writeln!(out, "Scenarios (seed {SEED})")?;
    writeln!(
        out,
        "{:<16} {:<18} {:>8} {:>9} {:>8} {:>10} {:>9} {:>10} {:>10} {:>8} {:>9}",
        "scenario",
        "arbiter",
        "frames",
        "over %",
        "worst",
        "quality",
        "rows/min",
        "quote err",
        "decide",
        "max",
        "refused"
    )?;
    for scenario in Scenario::all() {
        for arbiters in Arbiters::ALL {
            let m = scenario.run(arbiters, SEED);
            let over = if m.frames == 0 {
                0.0
            } else {
                100.0 * m.frames_over as f64 / m.frames as f64
            };
            writeln!(
                out,
                "{:<16} {:<18} {:>8} {:>8.2}% {:>8} {:>10.3} {:>9.2} {:>10.3} {:>10} {:>8} {:>9}",
                scenario.name,
                arbiters.name(),
                m.frames,
                over,
                ms(m.worst_frame),
                m.quality_mean,
                m.row_changes_per_minute,
                m.prediction_error,
                us(m.decision_time_mean),
                us(m.decision_time_max),
                m.refusals,
            )?;
        }
    }
    Ok(())
}

/// `trace` decided again by every proposer set.
fn journal(out: &mut impl Write, name: &str, trace: &DecisionTrace) -> io::Result<()> {
    writeln!(out, "Journal {name}: {} epochs", trace.epochs.len())?;
    writeln!(
        out,
        "{:<18} {:>10} {:>10} {:>9} {:>10} {:>10} {:>8} {:>9}",
        "arbiter", "quality", "slack", "rows/min", "as rec.", "decide", "max", "refused"
    )?;
    let mut frames = None;
    for arbiters in Arbiters::ALL {
        let m = replay(trace, arbiters).metrics;
        writeln!(
            out,
            "{:<18} {:>10.3} {:>8.2}ms {:>9} {:>10} {:>10} {:>8} {:>9}",
            arbiters.name(),
            m.quality_mean,
            m.slack_mean_ms,
            m.row_changes_per_minute
                .map_or_else(|| "-".to_owned(), |rate| format!("{rate:.2}")),
            format!("{}/{}", m.same_as_recorded, m.epochs),
            us(m.decision_time_mean),
            us(m.decision_time_max),
            m.refusals,
        )?;
        frames = Some((m.frames, m.frames_over, m.worst_frame, m.prediction_error));
    }
    // What the journal says of the run itself, whatever the proposers.
    if let Some((count, over, worst, error)) = frames {
        writeln!(
            out,
            "recorded run: {count} frames, {over} over their target, worst {}; quote error {}",
            ms(worst),
            error.map_or_else(|| "-".to_owned(), |error| format!("{error:.3}")),
        )?;
    }
    Ok(())
}

fn ms(time: Duration) -> String {
    format!("{:.1}ms", time.as_secs_f64() * 1_000.0)
}

fn us(time: Duration) -> String {
    format!("{}µs", time.as_micros())
}
