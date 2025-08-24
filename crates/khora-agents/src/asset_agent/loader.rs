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
use khora_lanes::asset_lane::AssetLoader;
use std::{any::Any, collections::HashMap};

/// Internal trait for loading any asset type.
trait AnyLoader: Send + Sync {
    fn load_any(&self, bytes: &[u8]) -> Result<Box<dyn Any + Send>>;
}

/// A "wrapper" that takes a generic `AssetLoader<A>` and implements `AnyLoader`.
struct LoaderWrapper<A: Asset, L: AssetLoader<A>>(L, std::marker::PhantomData<A>);

impl<A: Asset, L: AssetLoader<A> + Send + Sync> AnyLoader for LoaderWrapper<A, L> {
    fn load_any(&self, bytes: &[u8]) -> Result<Box<dyn Any + Send>> {
        // Call the GENERIC and TYPE-SAFE load() method...
        let asset: A = self.0.load(bytes).map_err(|e| anyhow!(e.to_string()))?;
        // ...and return the result in a Box<dyn Any>.
        Ok(Box::new(asset))
    }
}

/// The registry that manages complexity for the AssetAgent.
pub(crate) struct LoaderRegistry {
    loaders: HashMap<String, Box<dyn AnyLoader>>,
}

impl LoaderRegistry {
    /// Creates a new `LoaderRegistry`.
    pub(crate) fn new() -> Self {
        Self {
            loaders: HashMap::new(),
        }
    }

    /// Registers a new asset loader.
    pub(crate) fn register<A: Asset>(
        &mut self,
        type_name: &str,
        loader: impl AssetLoader<A> + Send + Sync + 'static,
    ) {
        let wrapped = LoaderWrapper(loader, std::marker::PhantomData);
        self.loaders
            .insert(type_name.to_string(), Box::new(wrapped));
    }

    /// Loads an asset of the specified type from raw bytes.
    pub(crate) fn load<A: Asset>(&self, type_name: &str, bytes: &[u8]) -> Result<A> {
        let loader = self
            .loaders
            .get(type_name)
            .ok_or_else(|| anyhow!("No loader registered for asset type '{}'", type_name))?;

        let asset_any = loader.load_any(bytes)?;

        let asset_boxed = asset_any.downcast::<A>().map_err(|_| {
            anyhow!(
                "Loader for type '{}' returned a different asset type than requested.",
                type_name
            )
        })?;

        Ok(*asset_boxed)
    }
}
