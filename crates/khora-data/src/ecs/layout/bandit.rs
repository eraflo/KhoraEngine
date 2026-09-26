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

//! Multi-armed bandits (UCB1, sliding-window UCB1) and the switching rule the
//! layout advisor uses to pick a layout.

use std::collections::VecDeque;

/// Per-arm running statistics for the bandit.
#[derive(Debug, Clone, Copy, Default)]
struct ArmStats {
    /// How many times this arm has been pulled.
    pulls: u64,
    /// Running mean of the rewards observed for this arm.
    mean: f64,
}

/// A deterministic **UCB1** multi-armed bandit.
///
/// Balances *exploration* (try arms whose value is uncertain) and *exploitation*
/// (favour the arm with the best observed reward) using the classic upper
/// confidence bound `x̄ᵢ + c·√(ln N / nᵢ)`. Unpulled arms are tried first. No
/// randomness, so the same reward stream always yields the same decisions —
/// which is what makes the adaptive layer reproducible.
#[derive(Debug, Clone)]
pub struct Ucb1 {
    arms: Vec<ArmStats>,
    total: u64,
    exploration: f64,
}

impl Ucb1 {
    /// Default exploration constant `c = √2`, the textbook UCB1 value.
    pub const DEFAULT_EXPLORATION: f64 = std::f64::consts::SQRT_2;

    /// Creates a bandit with `num_arms` arms (clamped to at least 1) and the
    /// default exploration constant.
    pub fn new(num_arms: usize) -> Self {
        Self::with_exploration(num_arms, Self::DEFAULT_EXPLORATION)
    }

    /// Creates a bandit with an explicit exploration constant `c` (higher = more
    /// exploration).
    pub fn with_exploration(num_arms: usize, exploration: f64) -> Self {
        Self {
            arms: vec![ArmStats::default(); num_arms.max(1)],
            total: 0,
            exploration,
        }
    }

    /// The number of arms.
    pub fn arm_count(&self) -> usize {
        self.arms.len()
    }

    /// Number of times `arm` has been pulled (0 if out of range).
    pub fn pulls(&self, arm: usize) -> u64 {
        self.arms.get(arm).map(|a| a.pulls).unwrap_or(0)
    }

    /// Selects the next arm to try. Any never-pulled arm is chosen first (forced
    /// initial exploration); otherwise the arm with the highest UCB score.
    pub fn select(&self) -> usize {
        // Forced exploration: pull each arm once before scoring.
        if let Some(i) = self.arms.iter().position(|a| a.pulls == 0) {
            return i;
        }
        let ln_total = (self.total as f64).max(1.0).ln();
        let mut best = 0usize;
        let mut best_score = f64::NEG_INFINITY;
        for (i, arm) in self.arms.iter().enumerate() {
            let bonus = self.exploration * (ln_total / arm.pulls as f64).sqrt();
            let score = arm.mean + bonus;
            if score > best_score {
                best_score = score;
                best = i;
            }
        }
        best
    }

    /// Records a `reward` for `arm` (incremental mean update). Out-of-range arms
    /// and non-finite rewards are ignored.
    pub fn record(&mut self, arm: usize, reward: f64) {
        if !reward.is_finite() {
            return;
        }
        let Some(stats) = self.arms.get_mut(arm) else {
            return;
        };
        stats.pulls += 1;
        // Welford-style incremental mean.
        stats.mean += (reward - stats.mean) / stats.pulls as f64;
        self.total += 1;
    }

    /// The current best arm by observed mean reward (pure exploitation), or
    /// `None` until at least one arm has been pulled.
    pub fn best_arm(&self) -> Option<usize> {
        self.arms
            .iter()
            .enumerate()
            .filter(|(_, a)| a.pulls > 0)
            .max_by(|(_, a), (_, b)| {
                a.mean
                    .partial_cmp(&b.mean)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i)
    }

    /// The observed mean reward for `arm`, or `None` if it hasn't been pulled.
    pub fn mean_reward(&self, arm: usize) -> Option<f64> {
        self.arms.get(arm).filter(|a| a.pulls > 0).map(|a| a.mean)
    }
}

/// One arm of a [`SlidingWindowUcb1`] — the most recent `window` rewards and
/// their running sum (so the mean is `O(1)` to read).
#[derive(Debug, Clone, Default)]
struct WindowArm {
    rewards: VecDeque<f64>,
    sum: f64,
}

impl WindowArm {
    fn record(&mut self, reward: f64, window: usize) {
        self.rewards.push_back(reward);
        self.sum += reward;
        while self.rewards.len() > window {
            if let Some(old) = self.rewards.pop_front() {
                self.sum -= old;
            }
        }
    }

    fn count(&self) -> usize {
        self.rewards.len()
    }

    fn mean(&self) -> f64 {
        if self.rewards.is_empty() {
            0.0
        } else {
            self.sum / self.rewards.len() as f64
        }
    }
}

/// A **sliding-window** UCB1 bandit for *non-stationary* rewards.
///
/// Plain [`Ucb1`] assumes the reward distribution never changes, so its
/// confidence bound shrinks as `1/√n` forever and old samples pin the estimate
/// — which is wrong for a game session, where the best layout in a quiet scene
/// is not the best in a crowded one. This variant scores each arm over only its
/// most recent `window` rewards (a per-arm ring buffer), so it keeps adapting
/// when the workload shifts. It is still fully deterministic (no RNG), matching
/// the reproducibility the adaptive layer requires.
///
/// (Garivier & Moulines, *On Upper-Confidence Bound Policies for Non-Stationary
/// Bandit Problems*, 2011 — the SW-UCB construction.)
#[derive(Debug, Clone)]
pub struct SlidingWindowUcb1 {
    arms: Vec<WindowArm>,
    window: usize,
    exploration: f64,
}

impl SlidingWindowUcb1 {
    /// Creates a bandit with `num_arms` arms (min 1), each scored over its last
    /// `window` rewards (min 1), using the default exploration constant `√2`.
    pub fn new(num_arms: usize, window: usize) -> Self {
        Self::with_exploration(num_arms, window, Ucb1::DEFAULT_EXPLORATION)
    }

    /// Creates a bandit with an explicit exploration constant `c`.
    pub fn with_exploration(num_arms: usize, window: usize, exploration: f64) -> Self {
        Self {
            arms: vec![WindowArm::default(); num_arms.max(1)],
            window: window.max(1),
            exploration,
        }
    }

    /// The number of arms.
    pub fn arm_count(&self) -> usize {
        self.arms.len()
    }

    /// How many rewards are currently retained for `arm` (capped at the window).
    pub fn samples(&self, arm: usize) -> usize {
        self.arms.get(arm).map(|a| a.count()).unwrap_or(0)
    }

    /// Selects the next arm: any arm with an empty window first (forced
    /// exploration), otherwise the highest windowed UCB score.
    pub fn select(&self) -> usize {
        if let Some(i) = self.arms.iter().position(|a| a.count() == 0) {
            return i;
        }
        let total: usize = self.arms.iter().map(|a| a.count()).sum();
        let ln_total = (total as f64).max(1.0).ln();
        let mut best = 0usize;
        let mut best_score = f64::NEG_INFINITY;
        for (i, arm) in self.arms.iter().enumerate() {
            let bonus = self.exploration * (ln_total / arm.count() as f64).sqrt();
            let score = arm.mean() + bonus;
            if score > best_score {
                best_score = score;
                best = i;
            }
        }
        best
    }

    /// Records a `reward` for `arm` into its window (evicting the oldest once
    /// full). Out-of-range arms and non-finite rewards are ignored.
    pub fn record(&mut self, arm: usize, reward: f64) {
        if !reward.is_finite() {
            return;
        }
        let window = self.window;
        if let Some(stats) = self.arms.get_mut(arm) {
            stats.record(reward, window);
        }
    }

    /// The best arm by current windowed mean (pure exploitation), or `None`
    /// until at least one arm has a reward.
    pub fn best_arm(&self) -> Option<usize> {
        self.arms
            .iter()
            .enumerate()
            .filter(|(_, a)| a.count() > 0)
            .max_by(|(_, a), (_, b)| {
                a.mean()
                    .partial_cmp(&b.mean())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i)
    }

    /// The current windowed mean reward for `arm`, or `None` if its window is
    /// empty.
    pub fn mean_reward(&self, arm: usize) -> Option<f64> {
        self.arms
            .get(arm)
            .filter(|a| a.count() > 0)
            .map(|a| a.mean())
    }
}

/// Net reward of running under a layout once the cost of *getting there* is
/// charged against it: `benefit − migration_cost`.
///
/// Feeding this (rather than raw benefit) to a bandit is what makes switching
/// layouts self-limiting — a challenger has to beat the incumbent by *more than
/// the repack it would cost*, so a learner cannot thrash between layouts for a
/// marginal gain. (The cost-into-reward form from contextual index-tuning
/// bandits; the same idea as ski-rental "amortise the one-time cost".)
#[inline]
pub fn net_reward(benefit: f64, migration_cost: f64) -> f64 {
    benefit - migration_cost
}

/// Whether a challenger layout should displace the incumbent, given a
/// `hysteresis` margin it must clear: `challenger > incumbent + hysteresis`.
///
/// The margin is the anti-thrash / dwell mechanism (set it to the amortised
/// repack cost): without it a controller chatters between two near-equal
/// layouts, paying the switch cost each time. (OREO's α-gate and switched-system
/// hysteresis converge on the same "switch rarely" rule.)
#[inline]
pub fn should_switch(incumbent_score: f64, challenger_score: f64, hysteresis: f64) -> bool {
    challenger_score > incumbent_score + hysteresis.max(0.0)
}
