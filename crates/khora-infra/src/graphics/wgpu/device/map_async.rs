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

//! A future over a buffer's asynchronous map.

use khora_core::renderer::ResourceError;

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Poll, Waker};

pub(super) struct MapAsyncFutureState {
    pub(super) result: Mutex<Option<Result<(), ResourceError>>>,
    // The Waker to wake up the Future when the result is ready
    pub(super) waker: Mutex<Option<Waker>>,
}

// Custom Future implementation to wrap the MapAsyncFutureState
pub(super) struct MapAsyncOperationFuture {
    pub(super) state: Arc<MapAsyncFutureState>,
}

impl Future for MapAsyncOperationFuture {
    type Output = Result<(), ResourceError>;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let mut result_guard = match self.state.result.lock() {
            Ok(g) => g,
            Err(_) => {
                return Poll::Ready(Err(ResourceError::BackendError(
                    "MapAsyncOperationFuture: result mutex poisoned".to_owned(),
                )))
            }
        };
        if let Some(res) = result_guard.take() {
            Poll::Ready(res)
        } else {
            // Store the waker so the callback can wake this Future later.
            // If the waker mutex is poisoned, the callback won't be able
            // to schedule us — treat that as a fatal map_async failure.
            match self.state.waker.lock() {
                Ok(mut waker_guard) => {
                    *waker_guard = Some(cx.waker().clone());
                    Poll::Pending
                }
                Err(_) => Poll::Ready(Err(ResourceError::BackendError(
                    "MapAsyncOperationFuture: waker mutex poisoned".to_owned(),
                ))),
            }
        }
    }
}
