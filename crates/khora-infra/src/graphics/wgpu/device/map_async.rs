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

/// What a buffer's map callback shares with the future awaiting it.
pub(super) struct MapAsyncFutureState {
    pub(super) result: Mutex<Option<Result<(), ResourceError>>>,
    pub(super) waker: Mutex<Option<Waker>>,
}

/// A future that resolves when the map callback reports its result.
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
            // A poisoned waker mutex means the callback can never wake this
            // future: fail the map rather than hang.
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
