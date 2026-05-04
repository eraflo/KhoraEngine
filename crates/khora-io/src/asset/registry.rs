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
use anyhow::{anyhow, Result};
use khora_core::asset::Asset;
use khora_telemetry::{
    metrics::registry::{CounterHandle, HistogramHandle},
    MetricsRegistry, ScopedMetricTimer,
};
use std::{any::Any, collections::HashMap, sync::Arc};

trait AnyDecoder: Send + Sync {
    fn decode_any(&self, bytes: &[u8], metrics: &DecoderMetrics) -> Result<Box<dyn Any + Send>>;
}

struct DecoderWrapper<A: Asset, L: AssetDecoder<A>>(L, std::marker::PhantomData<A>);

impl<A: Asset, L: AssetDecoder<A> + Send + Sync> AnyDecoder for DecoderWrapper<A, L> {
    fn decode_any(&self, bytes: &[u8], metrics: &DecoderMetrics) -> Result<Box<dyn Any + Send>> {
        let _timer = ScopedMetricTimer::new(&metrics.decode_time_ms);
        let asset: A = self.0.load(bytes).map_err(|e| anyhow!(e.to_string()))?;
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
pub struct DecoderRegistry {
    metrics: DecoderMetrics,
    decoders: HashMap<String, Box<dyn AnyDecoder>>,
}

impl DecoderRegistry {
    /// Creates a new decoder registry.
    pub fn new(metrics_registry: Arc<MetricsRegistry>) -> Self {
        Self {
            decoders: HashMap::new(),
            metrics: DecoderMetrics::new(&metrics_registry),
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
            .insert(type_name.to_string(), Box::new(wrapped));
    }

    /// Decodes an asset of the specified type from raw bytes.
    pub fn decode<A: Asset>(&self, type_name: &str, bytes: &[u8]) -> Result<A> {
        let decoder = self
            .decoders
            .get(type_name)
            .ok_or_else(|| anyhow!("No decoder registered for asset type '{}'", type_name))?;

        let asset_any = decoder.decode_any(bytes, &self.metrics)?;
        let asset_boxed = asset_any.downcast::<A>().map_err(|_| {
            anyhow!(
                "Decoder for type '{}' returned a different asset type than requested.",
                type_name
            )
        })?;
        Ok(*asset_boxed)
    }
}
