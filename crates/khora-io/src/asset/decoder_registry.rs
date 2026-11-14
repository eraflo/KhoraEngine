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

//! Decoder registry — type-erased dispatch for asset decoding.

use super::AssetDecoder;
use crate::trust::{LimitExceeded, Limits, Trust};
use anyhow::{anyhow, Result};
use khora_core::asset::Asset;
use khora_telemetry::metrics::{CounterHandle, HistogramHandle};
use khora_telemetry::{MetricsRegistry, ScopedMetricTimer};
use std::{any::Any, collections::HashMap, path::Path, sync::Arc};

pub use super::decoder_registration::DecoderRegistration;

trait AnyDecoder: Send + Sync {
    fn sniff(&self, head: &[u8]) -> bool;

    fn limits(&self, trust: Trust) -> Limits;

    fn decode_any_limited(
        &self,
        bytes: &[u8],
        source_path: &Path,
        limits: &Limits,
        metrics: &DecoderMetrics,
    ) -> Result<Box<dyn Any + Send>>;

    fn decode_any(
        &self,
        bytes: &[u8],
        source_path: &Path,
        metrics: &DecoderMetrics,
    ) -> Result<Box<dyn Any + Send>>;
}

struct DecoderWrapper<A: Asset, L: AssetDecoder<A>>(L, std::marker::PhantomData<A>);

impl<A: Asset, L: AssetDecoder<A> + Send + Sync> AnyDecoder for DecoderWrapper<A, L> {
    fn sniff(&self, head: &[u8]) -> bool {
        self.0.sniff(head)
    }

    fn limits(&self, trust: Trust) -> Limits {
        self.0.limits(trust)
    }

    fn decode_any_limited(
        &self,
        bytes: &[u8],
        source_path: &Path,
        limits: &Limits,
        metrics: &DecoderMetrics,
    ) -> Result<Box<dyn Any + Send>> {
        let _timer = ScopedMetricTimer::new(&metrics.decode_time_ms);
        let asset: A = self
            .0
            .load_limited(bytes, source_path, limits)
            .map_err(error_of)?;
        metrics.assets_decoded_total.increment()?;
        Ok(Box::new(asset))
    }

    fn decode_any(
        &self,
        bytes: &[u8],
        source_path: &Path,
        metrics: &DecoderMetrics,
    ) -> Result<Box<dyn Any + Send>> {
        let _timer = ScopedMetricTimer::new(&metrics.decode_time_ms);
        let asset: A = self.0.load_at(bytes, source_path).map_err(error_of)?;
        metrics.assets_decoded_total.increment()?;
        Ok(Box::new(asset))
    }
}

struct DecoderMetrics {
    decode_time_ms: HistogramHandle,
    assets_decoded_total: CounterHandle,
}

impl DecoderMetrics {
    fn new(registry: &MetricsRegistry) -> Self {
        Self {
            decode_time_ms: registry
                .register_histogram(
                    "assets",
                    "decode_time",
                    "Asset decoding time",
                    "ms",
                    vec![1.0, 5.0, 16.0, 33.0, 100.0, 500.0],
                )
                .expect("Failed to register asset decode time metric"),
            assets_decoded_total: registry
                .register_counter("assets", "decoded_total", "Total number of assets decoded")
                .expect("Failed to register asset count metric"),
        }
    }
}

/// Registry of asset decoders, keyed by type name.
#[derive(Clone)]
pub struct DecoderRegistry {
    metrics: Arc<DecoderMetrics>,
    decoders: HashMap<String, Arc<dyn AnyDecoder>>,
}

impl DecoderRegistry {
    /// Creates a new decoder registry.
    pub fn new(metrics_registry: Arc<MetricsRegistry>) -> Self {
        Self {
            decoders: HashMap::new(),
            metrics: Arc::new(DecoderMetrics::new(&metrics_registry)),
        }
    }

    /// Registers a decoder for a specific asset type name.
    pub fn register<A: Asset>(
        &mut self,
        type_name: &str,
        decoder: impl AssetDecoder<A> + Send + Sync + 'static,
    ) {
        let wrapped = DecoderWrapper(decoder, std::marker::PhantomData);
        self.decoders
            .insert(type_name.to_string(), Arc::new(wrapped));
    }

    /// Decodes an asset of the specified type from raw bytes.
    pub fn decode<A: Asset>(&self, type_name: &str, bytes: &[u8]) -> Result<A> {
        self.decode_at(type_name, bytes, Path::new(""))
    }

    /// Decodes an asset of the specified type from the bytes of the file at
    /// `source_path` (relative to the assets root), for formats that read
    /// files next to their own.
    pub fn decode_at<A: Asset>(
        &self,
        type_name: &str,
        bytes: &[u8],
        source_path: &Path,
    ) -> Result<A> {
        let asset_any = self.decode_any_at(type_name, bytes, source_path)?;
        let asset_boxed = asset_any.downcast::<A>().map_err(|_| {
            anyhow!(
                "Decoder for type '{}' returned a different asset type than requested.",
                type_name
            )
        })?;
        Ok(*asset_boxed)
    }

    /// Whether the decoder registered for `type_name` reads a file starting
    /// with `head`; `None` when none is registered.
    pub fn sniff(&self, type_name: &str, head: &[u8]) -> Option<bool> {
        self.decoders
            .get(type_name)
            .map(|decoder| decoder.sniff(head))
    }

    /// The limits of a file of `type_name` read at `trust`, as its decoder
    /// gives them; `None` when no decoder is registered for it.
    pub fn limits(
        &self,
        type_name: &str,
        trust: crate::trust::Trust,
    ) -> Option<crate::trust::Limits> {
        self.decoders
            .get(type_name)
            .map(|decoder| decoder.limits(trust))
    }

    /// Decodes the bytes of the file at `source_path` with the decoder
    /// registered for `type_name`, within `limits`, as whatever type it
    /// produces. A limit that stopped it is kept in the error, as a
    /// [`LimitExceeded`].
    pub(crate) fn decode_any_limited(
        &self,
        type_name: &str,
        bytes: &[u8],
        source_path: &Path,
        limits: &Limits,
    ) -> Result<Box<dyn Any + Send>> {
        let decoder = self
            .decoders
            .get(type_name)
            .ok_or_else(|| anyhow!("No decoder registered for asset type '{}'", type_name))?;
        decoder.decode_any_limited(bytes, source_path, limits, &self.metrics)
    }
}

/// A decoder's error as the service carries it: a limit that stopped the
/// decoder stays a [`LimitExceeded`], what the reader refuses it by; any
/// other error by its message.
fn error_of(error: Box<dyn std::error::Error + Send + Sync>) -> anyhow::Error {
    match error.downcast::<LimitExceeded>() {
        Ok(limit) => anyhow::Error::new(*limit),
        Err(error) => anyhow!(error.to_string()),
    }
}

impl DecoderRegistry {
    /// Decodes the bytes of the file at `source_path` with the decoder
    /// registered for `type_name`, as whatever type it produces.
    pub(crate) fn decode_any_at(
        &self,
        type_name: &str,
        bytes: &[u8],
        source_path: &Path,
    ) -> Result<Box<dyn Any + Send>> {
        let decoder = self
            .decoders
            .get(type_name)
            .ok_or_else(|| anyhow!("No decoder registered for asset type '{}'", type_name))?;
        decoder.decode_any(bytes, source_path, &self.metrics)
    }
}
