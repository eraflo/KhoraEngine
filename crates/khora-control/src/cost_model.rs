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

//! Empirical cost model — fits a subsystem's measured runtime to a complexity
//! class so the DCC can *anticipate* a budget breach instead of only reacting to
//! it.
//!
//! Big-O cannot be derived from arbitrary code, so the engine **measures** it:
//! given `(n, time)` samples (workload size vs observed milliseconds), it fits a
//! constant factor `c` to each candidate class `f(n)` by least squares and keeps
//! the best-fitting one. The prediction is then `cost(n) ≈ c · f(n)` — the same
//! shape a database query optimiser uses (cardinality × per-row cost). This
//! learns the cost *on the hardware it actually runs on*, instead of trusting a
//! compile-time guess, and lets the cold path forecast "at this growth rate the
//! frame budget breaks at ~N entities".
//!
//! This module is the pure, dependency-free *algorithm*. Feeding it live samples
//! from telemetry and consuming its forecast in arbitration is the integration
//! step performed by the DCC.

/// A candidate complexity class the cost model can fit a measured workload to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComplexityClass {
    /// `f(n) = 1` — cost independent of workload size.
    Constant,
    /// `f(n) = n` — linear in workload size.
    Linear,
    /// `f(n) = n · log₂(n)` — linearithmic.
    Linearithmic,
    /// `f(n) = n²` — quadratic.
    Quadratic,
}

impl ComplexityClass {
    /// Every class considered by the fitter.
    pub const ALL: [ComplexityClass; 4] = [
        ComplexityClass::Constant,
        ComplexityClass::Linear,
        ComplexityClass::Linearithmic,
        ComplexityClass::Quadratic,
    ];

    /// Evaluates the (unscaled) growth function `f(n)` for this class.
    pub fn f(self, n: f64) -> f64 {
        let n = n.max(0.0);
        match self {
            ComplexityClass::Constant => 1.0,
            ComplexityClass::Linear => n,
            ComplexityClass::Linearithmic => {
                if n <= 1.0 {
                    n
                } else {
                    n * n.log2()
                }
            }
            ComplexityClass::Quadratic => n * n,
        }
    }
}

/// One observation: a workload size and the time it took (milliseconds).
#[derive(Debug, Clone, Copy)]
pub struct CostSample {
    /// Workload size (entities, bodies, draws, …).
    pub n: f64,
    /// Observed cost in milliseconds.
    pub time_ms: f64,
}

/// A rolling empirical cost model for a single subsystem (agent / strategy).
///
/// Records `(n, time)` samples in a bounded ring buffer and fits `c · f(n)` over
/// the candidate [`ComplexityClass`]es. Pure and dependency-free.
#[derive(Debug, Clone)]
pub struct CostModel {
    samples: Vec<CostSample>,
    capacity: usize,
    next: usize,
}

impl CostModel {
    /// Creates a model that keeps the most recent `capacity` samples (min 1).
    pub fn new(capacity: usize) -> Self {
        Self {
            samples: Vec::new(),
            capacity: capacity.max(1),
            next: 0,
        }
    }

    /// Records one `(n, time_ms)` observation. Overwrites the oldest sample once
    /// the ring is full. Non-finite inputs are ignored.
    pub fn record(&mut self, n: f64, time_ms: f64) {
        if !n.is_finite() || !time_ms.is_finite() {
            return;
        }
        let sample = CostSample { n, time_ms };
        if self.samples.len() < self.capacity {
            self.samples.push(sample);
        } else {
            self.samples[self.next] = sample;
            self.next = (self.next + 1) % self.capacity;
        }
    }

    /// Number of samples currently held.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether no samples have been recorded.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Fits each complexity class by least squares and returns the best
    /// `(class, c)` — the class with the smallest residual and the constant
    /// factor that scales its `f(n)` to the data.
    ///
    /// Returns `None` until there are at least two samples with distinct `n`
    /// (a single workload size can't distinguish the classes).
    pub fn best_fit(&self) -> Option<(ComplexityClass, f64)> {
        if self.samples.len() < 2 {
            return None;
        }
        let n0 = self.samples[0].n;
        if self.samples.iter().all(|s| s.n == n0) {
            return None;
        }

        // (class, c, residual)
        let mut best: Option<(ComplexityClass, f64, f64)> = None;
        for class in ComplexityClass::ALL {
            // Least-squares `c` minimising Σ(c·fᵢ − tᵢ)²  ⇒  c = Σ(fᵢ·tᵢ) / Σ(fᵢ²).
            let mut num = 0.0;
            let mut den = 0.0;
            for s in &self.samples {
                let f = class.f(s.n);
                num += f * s.time_ms;
                den += f * f;
            }
            if den <= f64::EPSILON {
                continue;
            }
            let c = num / den;
            let residual: f64 = self
                .samples
                .iter()
                .map(|s| {
                    let err = c * class.f(s.n) - s.time_ms;
                    err * err
                })
                .sum();
            if best.is_none_or(|(_, _, r)| residual < r) {
                best = Some((class, c, residual));
            }
        }
        best.map(|(class, c, _)| (class, c))
    }

    /// Predicts the cost in milliseconds at workload size `n` from the best fit.
    /// `None` until [`best_fit`](Self::best_fit) is available.
    pub fn predict_ms(&self, n: f64) -> Option<f64> {
        self.best_fit().map(|(class, c)| c * class.f(n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_linear_and_predicts() {
        let mut m = CostModel::new(16);
        for n in [10.0, 20.0, 40.0, 80.0] {
            m.record(n, 3.0 * n);
        }
        let (class, c) = m.best_fit().expect("fit available");
        assert_eq!(class, ComplexityClass::Linear);
        assert!((c - 3.0).abs() < 1e-6, "c = {c}");
        // Extrapolate beyond the observed range.
        assert!((m.predict_ms(100.0).unwrap() - 300.0).abs() < 1e-3);
    }

    #[test]
    fn fits_quadratic() {
        let mut m = CostModel::new(16);
        for n in [2.0, 4.0, 8.0, 16.0] {
            m.record(n, 0.5 * n * n);
        }
        assert_eq!(m.best_fit().unwrap().0, ComplexityClass::Quadratic);
    }

    #[test]
    fn needs_two_distinct_sizes() {
        let mut m = CostModel::new(16);
        m.record(10.0, 5.0);
        m.record(10.0, 5.0);
        assert!(m.best_fit().is_none());
    }

    #[test]
    fn ring_buffer_is_bounded() {
        let mut m = CostModel::new(2);
        m.record(1.0, 1.0);
        m.record(2.0, 2.0);
        m.record(3.0, 3.0);
        assert_eq!(m.len(), 2);
    }
}
