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

//! A registry for asset loaders, enabling dynamic loading of different asset types by name.

use anyhow::{anyhow, Result};
use khora_core::asset::Asset;
use khora_lanes::asset_lane::AssetLoaderLane;
use khora_telemetry::{
    metrics::registry::{CounterHandle, HistogramHandle},
    MetricsRegistry, ScopedMetricTimer,
};
use std::{any::Any, collections::HashMap, sync::Arc};

/// Internal trait for loading any asset type.
trait AnyLoaderLane: Send + Sync {
    fn load_any(&self, bytes: &[u8], metrics: &LoaderMetrics) -> Result<Box<dyn Any + Send>>;
}

/// A "wrapper" that takes a generic `AssetLoader<A>` and implements `AnyLoader`.
struct AssetLoaderLaneWrapper<A: Asset, L: AssetLoaderLane<A>>(L, std::marker::PhantomData<A>);

impl<A: Asset, L: AssetLoaderLane<A> + Send + Sync> AnyLoaderLane for AssetLoaderLaneWrapper<A, L> {
    fn load_any(&self, bytes: &[u8], metrics: &LoaderMetrics) -> Result<Box<dyn Any + Send>> {
        // Start the timer for this load operation.
        let _timer = ScopedMetricTimer::new(&metrics.load_time_ms);

        // Call the GENERIC and TYPE-SAFE load() method...
        let asset: A = self.0.load(bytes).map_err(|e| anyhow!(e.to_string()))?;

        // ...increment the asset loaded counter...
        metrics.assets_loaded_total.increment()?;

        // ...and return the result in a Box<dyn Any>.
        Ok(Box::new(asset))
    }
}

/// A collection of metric handles used by the loader registry.
struct LoaderMetrics {
    /// Histogram for tracking asset load times in milliseconds.
    load_time_ms: HistogramHandle,
    /// Counter for tracking the total number of assets loaded.
    assets_loaded_total: CounterHandle,
}

impl LoaderMetrics {
    fn new(registry: &MetricsRegistry) -> Self {
        Self {
            load_time_ms: registry
                .register_histogram(
                    "assets",
                    "load_time",
                    "Asset decoding time",
                    "ms",
                    vec![1.0, 5.0, 16.0, 33.0, 100.0, 500.0],
                )
                .expect("Failed to register asset load time metric"),
            assets_loaded_total: registry
                .register_counter(
                    "assets",
                    "loaded_total",
                    "Total number of assets loaded from disk",
                )
                .expect("Failed to register asset count metric"),
        }
    }
}

/// The registry that manages complexity for the AssetAgent.
pub(crate) struct AssetLoaderLaneRegistry {
    /// Metrics handles for monitoring loader performance.
    metrics: LoaderMetrics,
    /// A map from asset type names to their corresponding loaders.
    loaders: HashMap<String, Box<dyn AnyLoaderLane>>,
}

impl AssetLoaderLaneRegistry {
    /// Creates a new `LoaderRegistry`.
    pub(crate) fn new(metrics_registry: Arc<MetricsRegistry>) -> Self {
        Self {
            loaders: HashMap::new(),
            metrics: LoaderMetrics::new(&metrics_registry),
        }
    }

    /// Registers a new asset loader.
    pub(crate) fn register<A: Asset>(
        &mut self,
        type_name: &str,
        loader: impl AssetLoaderLane<A> + Send + Sync + 'static,
    ) {
        let wrapped = AssetLoaderLaneWrapper(loader, std::marker::PhantomData);
        self.loaders
            .insert(type_name.to_string(), Box::new(wrapped));
    }

    /// Loads an asset of the specified type from raw bytes.
    pub(crate) fn load<A: Asset>(&self, type_name: &str, bytes: &[u8]) -> Result<A> {
        let loader = self
            .loaders
            .get(type_name)
            .ok_or_else(|| anyhow!("No loader registered for asset type '{}'", type_name))?;

        let asset_any = loader.load_any(bytes, &self.metrics)?;

        let asset_boxed = asset_any.downcast::<A>().map_err(|_| {
            anyhow!(
                "Loader for type '{}' returned a different asset type than requested.",
                type_name
            )
        })?;

        Ok(*asset_boxed)
    }
}
