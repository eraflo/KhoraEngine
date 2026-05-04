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

//! Editor icon set — names and Unicode codepoints from the **Lucide** font.
//!
//! Each variant maps to a Private-Use-Area codepoint defined by `lucide.ttf`
//! (v0.469.0). Apps render an icon by passing
//! [`Icon::glyph()`] to [`UiBuilder::paint_text_styled`] with
//! [`FontFamilyHint::Icons`], or by using a higher-level widget that does
//! that internally.
//!
//! See <https://lucide.dev/icons/> for visual previews.

/// A semantic icon identifier mapped to a Lucide codepoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(missing_docs)]
pub enum Icon {
    // Navigation
    Search,
    ChevronDown,
    ChevronRight,
    ArrowRight,

    // 3D / scene
    Cube,
    Light,
    Camera,
    Folder,
    Layers,
    Globe,
    Axes,
    Grid,
    Sparkles,
    Package,

    // Transport / play
    Play,
    Pause,
    Stop,
    StepForward,

    // Visibility / lock
    Eye,
    EyeOff,
    Lock,

    // Common UI
    Plus,
    More,
    Filter,
    Trash,
    Bell,
    Settings,
    Share,
    Hammer,
    Branch,
    Menu,
    Tag,

    // Tools
    Move,
    Rotate,
    Scale,
    Hand,
    Crosshair,

    // Bottom panels
    Database,
    Film,
    Terminal,
    Code,
    Image,
    Music,
    Pen,

    // Status / log levels
    Info,
    Warn,
    Error,

    // Brand / accents
    Zap,
    Command,

    // Hardware
    Wifi,
    Cpu,
    Memory,

    // Misc
    Box,
    Dot,
    Circle,
    CheckCircle,
}

impl Icon {
    /// Returns the Lucide PUA glyph for this icon as a `&'static str`.
    /// Use it with `paint_text_styled(.., FontFamilyHint::Icons, ..)` or with
    /// any widget that renders icon text.
    pub fn glyph(self) -> &'static str {
        match self {
            // Navigation
            Self::Search => "\u{e154}",
            Self::ChevronDown => "\u{e071}",
            Self::ChevronRight => "\u{e073}",
            Self::ArrowRight => "\u{e04d}",

            // 3D / scene
            Self::Cube => "\u{e065}",  // box
            Self::Light => "\u{e1c1}", // lightbulb
            Self::Camera => "\u{e068}",
            Self::Folder => "\u{e0dc}",
            Self::Layers => "\u{e52d}",
            Self::Globe => "\u{e0eb}",
            Self::Axes => "\u{e2fd}",
            Self::Grid => "\u{e0ec}",
            Self::Sparkles => "\u{e416}",
            Self::Package => "\u{e12c}",

            // Transport
            Self::Play => "\u{e13f}",
            Self::Pause => "\u{e131}",
            Self::Stop => "\u{e16a}", // square
            Self::StepForward => "\u{e3ed}",

            // Visibility / lock
            Self::Eye => "\u{e0be}",
            Self::EyeOff => "\u{e0bf}",
            Self::Lock => "\u{e10e}",

            // Common UI
            Self::Plus => "\u{e140}",
            Self::More => "\u{e118}", // menu
            Self::Filter => "\u{e0d5}",
            Self::Trash => "\u{e18d}",
            Self::Bell => "\u{e05d}",
            Self::Settings => "\u{e157}",
            Self::Share => "\u{e159}",
            Self::Hammer => "\u{e0ef}",
            Self::Branch => "\u{e0e5}",
            Self::Menu => "\u{e118}",
            Self::Tag => "\u{e182}",

            // Tools
            Self::Move => "\u{e124}",
            Self::Rotate => "\u{e2e9}",
            Self::Scale => "\u{e211}",
            Self::Hand => "\u{e1d6}",
            Self::Crosshair => "\u{e0b0}",

            // Bottom panels
            Self::Database => "\u{e0b1}",
            Self::Film => "\u{e0d4}",
            Self::Terminal => "\u{e184}",
            Self::Code => "\u{e097}",
            Self::Image => "\u{e0f9}",
            Self::Music => "\u{e34d}",
            Self::Pen => "\u{e132}",

            // Status / log levels
            Self::Info => "\u{e0fe}",
            Self::Warn => "\u{e192}",  // triangle-alert
            Self::Error => "\u{e088}", // circle-x

            // Brand / accents
            Self::Zap => "\u{e1b3}",
            Self::Command => "\u{e09e}",

            // Hardware
            Self::Wifi => "\u{e1ad}",
            Self::Cpu => "\u{e0ad}",
            Self::Memory => "\u{e449}",

            // Misc
            Self::Box => "\u{e065}",
            Self::Dot => "\u{e453}",
            Self::Circle => "\u{e07a}",
            Self::CheckCircle => "\u{e225}",
        }
    }
}
