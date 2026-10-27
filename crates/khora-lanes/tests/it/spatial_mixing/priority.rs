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

//! A source's priority ranks before how loud it is: an author marks the
//! sources that must stay heard when voices run out.

use khora_data::flow::AudioView;

use super::fixtures::{assert_same_mix, constant, view_of, Mixer, Voice, ALL_VOICES, EIGHT_VOICES};

/// `view` with the source at `k` raised to `priority`.
fn with_priority(mut view: AudioView, k: usize, priority: u8) -> AudioView {
    view.sources[k].priority = priority;
    view
}

/// **A high-priority source keeps its voice, however quiet.** Nine sources
/// play at eight voices: eight loud, one quiet but raised above them all. The
/// quiet one is mixed, and the loud one of lowest rank — the last in View
/// order among equals — is the one that goes virtual.
#[test]
fn a_high_priority_source_keeps_its_voice_over_louder_ones() {
    let tone = constant(0.01, 8192);
    let mut voices: Vec<Voice> = (0..8).map(|_| Voice::playing(&tone, 1.0)).collect();
    voices.push(Voice::playing(&tone, 0.2));

    let capped = Mixer::mono().mix(EIGHT_VOICES, with_priority(view_of(None, &voices), 8, 200));
    let kept: Vec<Voice> = (0..7)
        .map(|k| voices[k].clone())
        .chain([voices[8].clone()])
        .collect();
    let reference = Mixer::mono().mix(ALL_VOICES, view_of(None, &kept));

    assert_same_mix(
        &capped.block,
        &reference.block,
        "the raised quiet source is mixed in place of the last loud one",
    );
}

/// A lower priority goes virtual first, however loud: a loud source the
/// author lowered gives its voice to a quieter one at the default priority.
#[test]
fn a_lowered_source_goes_virtual_first() {
    let tone = constant(0.01, 8192);
    let mut voices: Vec<Voice> = (0..8).map(|_| Voice::playing(&tone, 0.5)).collect();
    voices.push(Voice::playing(&tone, 1.0));

    let lowered = with_priority(view_of(None, &voices), 8, 10);
    let capped = Mixer::mono().mix(EIGHT_VOICES, lowered);
    let reference = Mixer::mono().mix(ALL_VOICES, view_of(None, &voices[..8]));

    assert_same_mix(
        &capped.block,
        &reference.block,
        "the lowered loud source is the one left out",
    );
}
