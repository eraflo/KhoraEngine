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

//! Borrows lent to a [`LaneContext`] for the lifetime `'a` of the context.
//!
//! An agent lends a mutable borrow with [`LaneContext::insert_slot`] (or
//! [`LaneContext::insert_slot_as`] under a tag type) and a shared borrow with
//! [`LaneContext::insert_ref`]. A lane takes the mutable borrow back as a
//! [`SlotGuard`] with [`LaneContext::slot`]; while that guard lives, a second
//! `slot` of the same key returns `None`.

use std::any::TypeId;
use std::cell::Cell;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;

use super::LaneContext;

/// The tag of a borrow lent with [`LaneContext::insert_slot`], as opposed to
/// [`LaneContext::insert_slot_as`].
struct Untagged;

/// A mutable borrow as the context stores it: the lifetime is carried by the
/// context (`LaneContext<'a>`), not by the entry, so the entry is `'static`
/// and can sit in the type map.
///
/// Private, so the owned-value accessors (`get`, `get_mut`, `remove`) can
/// never name it — the pointer cannot be moved out of the context and kept
/// past `'a`. The raw pointer also makes it neither `Send` nor `Sync`.
struct SlotEntry<Tag, T: ?Sized> {
    ptr: NonNull<T>,
    lent: Cell<bool>,
    _tag: PhantomData<fn() -> Tag>,
}

/// A shared borrow as the context stores it. Private for the same reason as
/// [`SlotEntry`].
struct RefEntry<T: ?Sized> {
    ptr: NonNull<T>,
}

/// Exclusive access to a mutable borrow lent to a [`LaneContext`].
///
/// Dereferences to the lent value. Dropping the guard releases the borrow so
/// the next [`LaneContext::slot`] of the same key returns it again; forgetting
/// the guard keeps it lent for the rest of the context's life.
pub struct SlotGuard<'b, T: ?Sized> {
    value: &'b mut T,
    lent: &'b Cell<bool>,
}

impl<T: ?Sized> Deref for SlotGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.value
    }
}

impl<T: ?Sized> DerefMut for SlotGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.value
    }
}

impl<T: ?Sized> Drop for SlotGuard<'_, T> {
    fn drop(&mut self) {
        self.lent.set(false);
    }
}

impl<'a> LaneContext<'a> {
    /// Lends a mutable borrow to the context, keyed by `T`, for the context's
    /// whole life. Replaces a borrow of the same key.
    ///
    /// The referent must outlive every use of the context:
    ///
    /// ```rust,compile_fail,E0597
    /// use khora_core::lane::LaneContext;
    ///
    /// let mut ctx = LaneContext::new();
    /// {
    ///     let mut value = 10u32;
    ///     ctx.insert_slot(&mut value);
    /// }
    /// let _ = ctx.slot::<u32>().is_some();
    /// ```
    ///
    /// and it cannot be touched while it is lent:
    ///
    /// ```rust,compile_fail,E0503
    /// use khora_core::lane::LaneContext;
    ///
    /// let mut value = 10u32;
    /// let mut ctx = LaneContext::new();
    /// ctx.insert_slot(&mut value);
    /// value += 1;
    /// let _ = ctx.slot::<u32>().is_some();
    /// ```
    pub fn insert_slot<T: ?Sized + 'static>(&mut self, value: &'a mut T) {
        self.insert_slot_as::<Untagged, T>(value);
    }

    /// Lends a mutable borrow to the context under the tag type `Tag`, so two
    /// borrows of the same `T` can coexist. A tagged borrow is reached only
    /// through [`slot_as`](Self::slot_as) with the same tag.
    pub fn insert_slot_as<Tag: 'static, T: ?Sized + 'static>(&mut self, value: &'a mut T) {
        let entry = SlotEntry::<Tag, T> {
            ptr: NonNull::from(value),
            lent: Cell::new(false),
            _tag: PhantomData,
        };
        self.data
            .insert(TypeId::of::<SlotEntry<Tag, T>>(), Box::new(entry));
    }

    /// Takes the mutable borrow lent under `T` with
    /// [`insert_slot`](Self::insert_slot).
    ///
    /// Returns `None` when nothing was lent under `T`, or when a guard for it
    /// is still alive. The guard cannot outlive the context:
    ///
    /// ```rust,compile_fail,E0597
    /// use khora_core::lane::LaneContext;
    ///
    /// let mut value = 10u32;
    /// let guard;
    /// {
    ///     let mut ctx = LaneContext::new();
    ///     ctx.insert_slot(&mut value);
    ///     guard = ctx.slot::<u32>();
    /// }
    /// drop(guard);
    /// ```
    pub fn slot<T: ?Sized + 'static>(&self) -> Option<SlotGuard<'_, T>> {
        self.slot_as::<Untagged, T>()
    }

    /// Takes the mutable borrow lent under the tag `Tag` with
    /// [`insert_slot_as`](Self::insert_slot_as). Same rules as
    /// [`slot`](Self::slot).
    pub fn slot_as<Tag: 'static, T: ?Sized + 'static>(&self) -> Option<SlotGuard<'_, T>> {
        let entry = self
            .data
            .get(&TypeId::of::<SlotEntry<Tag, T>>())?
            .downcast_ref::<SlotEntry<Tag, T>>()?;
        if entry.lent.replace(true) {
            return None;
        }
        // SAFETY: `ptr` was made from a `&'a mut T` handed to `insert_slot_as`,
        // so the referent is live and untouched by anyone else for `'a`, and
        // `'a` outlives this borrow of the context. `lent` was false and is now
        // true, so no other guard to it exists; it goes back to false only when
        // this guard drops. The entry cannot be replaced or removed while the
        // guard borrows the context.
        let value = unsafe { &mut *entry.ptr.as_ptr() };
        Some(SlotGuard {
            value,
            lent: &entry.lent,
        })
    }

    /// Lends a shared borrow to the context, keyed by `T`, for the context's
    /// whole life. Replaces a borrow of the same key.
    ///
    /// The referent must outlive every use of the context:
    ///
    /// ```rust,compile_fail,E0597
    /// use khora_core::lane::LaneContext;
    ///
    /// let mut ctx = LaneContext::new();
    /// {
    ///     let value = 10u32;
    ///     ctx.insert_ref(&value);
    /// }
    /// let _ = ctx.get_ref::<u32>().is_some();
    /// ```
    pub fn insert_ref<T: ?Sized + 'static>(&mut self, value: &'a T) {
        let entry = RefEntry::<T> {
            ptr: NonNull::from(value),
        };
        self.data
            .insert(TypeId::of::<RefEntry<T>>(), Box::new(entry));
    }

    /// Returns the shared borrow lent under `T` with
    /// [`insert_ref`](Self::insert_ref), for the context's lifetime `'a` — not
    /// tied to the borrow of the context itself.
    pub fn get_ref<T: ?Sized + 'static>(&self) -> Option<&'a T> {
        let entry = self
            .data
            .get(&TypeId::of::<RefEntry<T>>())?
            .downcast_ref::<RefEntry<T>>()?;
        // SAFETY: `ptr` was made from a `&'a T` handed to `insert_ref`; a shared
        // borrow is freely copyable, so handing it back for the same `'a` is
        // exactly what the caller lent.
        Some(unsafe { entry.ptr.as_ref() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TagA;
    struct TagB;

    trait Counter {
        fn bump(&mut self);
        fn value(&self) -> u32;
    }

    struct Count(u32);

    impl Counter for Count {
        fn bump(&mut self) {
            self.0 += 1;
        }

        fn value(&self) -> u32 {
            self.0
        }
    }

    #[test]
    fn slot_round_trip_mutates_referent() {
        let mut x = 10u32;
        {
            let mut ctx = LaneContext::new();
            ctx.insert_slot(&mut x);
            *ctx.slot::<u32>().expect("slot lent") = 20;
        }
        assert_eq!(x, 20);
    }

    #[test]
    fn slot_second_borrow_while_live_is_none() {
        let mut x = 1u32;
        let mut ctx = LaneContext::new();
        ctx.insert_slot(&mut x);

        let mut first = ctx.slot::<u32>().expect("first borrow");
        assert!(ctx.slot::<u32>().is_none());
        *first += 1;
        assert!(ctx.slot::<u32>().is_none());
        assert_eq!(*first, 2);
        drop(first);
        drop(ctx);
        assert_eq!(x, 2);
    }

    #[test]
    fn slot_available_again_after_guard_drop() {
        let mut x = 0u32;
        let mut ctx = LaneContext::new();
        ctx.insert_slot(&mut x);

        {
            let mut guard = ctx.slot::<u32>().expect("first borrow");
            *guard += 5;
        }
        {
            let mut guard = ctx.slot::<u32>().expect("borrow released on drop");
            assert_eq!(*guard, 5);
            *guard += 5;
        }
        assert!(ctx.slot::<u32>().is_some());
        drop(ctx);
        assert_eq!(x, 10);
    }

    #[test]
    fn slot_forgotten_guard_stays_lent() {
        let mut x = 3u32;
        let mut ctx = LaneContext::new();
        ctx.insert_slot(&mut x);

        let mut guard = ctx.slot::<u32>().expect("first borrow");
        *guard = 4;
        std::mem::forget(guard);

        assert!(ctx.slot::<u32>().is_none());
        assert!(ctx.slot::<u32>().is_none());
        drop(ctx);
        assert_eq!(x, 4);
    }

    #[test]
    fn slot_as_tags_are_distinct_keys() {
        let mut a = 1u32;
        let mut b = 2u32;
        let mut plain = 3u32;
        {
            let mut ctx = LaneContext::new();
            ctx.insert_slot_as::<TagA, u32>(&mut a);
            ctx.insert_slot_as::<TagB, u32>(&mut b);
            ctx.insert_slot(&mut plain);

            let mut ga = ctx.slot_as::<TagA, u32>().expect("tag A lent");
            let mut gb = ctx.slot_as::<TagB, u32>().expect("tag B lent");
            let mut gp = ctx.slot::<u32>().expect("untagged lent");
            assert_eq!((*ga, *gb, *gp), (1, 2, 3));
            *ga += 10;
            *gb += 20;
            *gp += 30;

            assert!(ctx.slot_as::<TagA, u32>().is_none());
            assert!(ctx.slot_as::<TagB, u32>().is_none());
            assert!(ctx.slot::<u32>().is_none());
            drop((ga, gb, gp));
        }
        assert_eq!((a, b, plain), (11, 22, 33));
    }

    #[test]
    fn slot_as_tag_not_reached_by_untagged_slot() {
        let mut a = 1u32;
        let mut ctx = LaneContext::new();
        ctx.insert_slot_as::<TagA, u32>(&mut a);

        assert!(ctx.slot::<u32>().is_none());
        assert!(ctx.slot_as::<TagB, u32>().is_none());
        assert!(ctx.slot_as::<TagA, u32>().is_some());
    }

    #[test]
    fn slot_unsized_dyn_trait() {
        let mut count = Count(0);
        {
            let mut ctx = LaneContext::new();
            let lent: &mut dyn Counter = &mut count;
            ctx.insert_slot::<dyn Counter>(lent);

            ctx.slot::<dyn Counter>().expect("dyn lent").bump();
            let mut guard = ctx.slot::<dyn Counter>().expect("released after bump");
            guard.bump();
            assert_eq!(guard.value(), 2);
        }
        assert_eq!(count.0, 2);
    }

    #[test]
    fn slot_unsized_slice() {
        let mut values = [1u32, 2, 3];
        {
            let mut ctx = LaneContext::new();
            ctx.insert_slot::<[u32]>(&mut values[..]);
            let mut guard = ctx.slot::<[u32]>().expect("slice lent");
            assert_eq!(guard.len(), 3);
            guard[1] = 20;
        }
        assert_eq!(values, [1, 20, 3]);
    }

    #[test]
    fn get_ref_outlives_context_borrow() {
        let text = String::from("hello");
        let numbers = [1u32, 2, 3];
        let mut counter = 0u32;

        let mut ctx = LaneContext::new();
        ctx.insert_ref::<String>(&text);
        ctx.insert_ref::<[u32]>(&numbers[..]);

        let text_back: &String = ctx.get_ref::<String>().expect("ref lent");
        let numbers_back: &[u32] = ctx.get_ref::<[u32]>().expect("slice lent");

        ctx.insert(7u64);
        ctx.insert_slot(&mut counter);
        *ctx.slot::<u32>().expect("slot lent") += 1;
        let _ = ctx.remove::<u64>();

        assert_eq!(text_back, "hello");
        assert_eq!(numbers_back, &[1, 2, 3]);
        assert!(std::ptr::eq(text_back, &text));
        assert_eq!(ctx.get_ref::<String>().map(String::as_str), Some("hello"));
        drop(ctx);
        assert_eq!(counter, 1);
    }

    #[test]
    fn borrows_invisible_to_owned_accessors() {
        let mut x = 5u32;
        let shared = 9u64;
        let mut ctx = LaneContext::new();
        ctx.insert_slot(&mut x);
        ctx.insert_ref(&shared);

        assert!(ctx.get::<u32>().is_none());
        assert!(ctx.get_mut::<u32>().is_none());
        assert!(!ctx.contains::<u32>());
        assert!(ctx.remove::<u32>().is_none());
        assert!(ctx.get::<u64>().is_none());
        assert!(!ctx.contains::<u64>());
        assert!(ctx.remove::<u64>().is_none());

        // The borrows survive the owned-value removals.
        *ctx.slot::<u32>().expect("slot still lent") = 6;
        assert_eq!(ctx.get_ref::<u64>(), Some(&9));
        drop(ctx);
        assert_eq!(x, 6);
    }

    #[test]
    fn owned_values_invisible_to_borrow_accessors() {
        let mut ctx = LaneContext::new();
        ctx.insert(1u32);
        ctx.insert(2u64);

        assert!(ctx.slot::<u32>().is_none());
        assert!(ctx.slot_as::<TagA, u32>().is_none());
        assert!(ctx.get_ref::<u32>().is_none());
        assert!(ctx.get_ref::<u64>().is_none());
        assert_eq!(ctx.get::<u32>(), Some(&1));
    }

    #[test]
    fn slot_and_ref_keys_are_distinct() {
        let mut lent = 1u32;
        let shared = 2u32;
        let mut ctx = LaneContext::new();
        ctx.insert_ref(&shared);
        assert!(ctx.slot::<u32>().is_none());

        ctx.insert_slot(&mut lent);
        assert_eq!(ctx.get_ref::<u32>(), Some(&2));
        assert_eq!(ctx.slot::<u32>().map(|guard| *guard), Some(1));
    }

    #[test]
    fn reinsert_same_key_replaces_borrow() {
        let mut first = 1u32;
        let mut second = 2u32;
        let shared_first = 10u64;
        let shared_second = 20u64;
        {
            let mut ctx = LaneContext::new();
            ctx.insert_slot(&mut first);
            // A guard forgotten on the first borrow must not leak into the
            // replacement's lent flag.
            std::mem::forget(ctx.slot::<u32>().expect("first lent"));
            ctx.insert_slot(&mut second);
            *ctx.slot::<u32>().expect("replacement lent, flag fresh") += 100;

            ctx.insert_ref(&shared_first);
            ctx.insert_ref(&shared_second);
            assert_eq!(ctx.get_ref::<u64>(), Some(&20));
        }
        assert_eq!((first, second), (1, 102));
    }

    #[test]
    fn slot_missing_is_none() {
        let ctx = LaneContext::new();
        assert!(ctx.slot::<u32>().is_none());
        assert!(ctx.slot::<dyn Counter>().is_none());
        assert!(ctx.slot_as::<TagA, u32>().is_none());
        assert!(ctx.get_ref::<u32>().is_none());
        assert!(ctx.get_ref::<str>().is_none());
    }
}

/// Soundness guarantees of the borrows lent to a [`LaneContext`], checked at
/// compile time.
///
/// A [`SlotGuard`] cannot be sent to another thread: its drop writes the
/// entry's `Cell<bool>` flag, which the owning thread reads in `slot`, so a
/// guard dropped elsewhere would race it.
///
/// ```rust,compile_fail,E0277
/// use khora_core::lane::SlotGuard;
///
/// fn assert_send<T: Send>() {}
/// assert_send::<SlotGuard<'static, u32>>();
/// ```
///
/// The `&'a T` returned by `get_ref` cannot outlive the referent it was lent
/// from:
///
/// ```rust,compile_fail,E0597
/// use khora_core::lane::LaneContext;
///
/// let escaped: Option<&u32>;
/// {
///     let value = 10u32;
///     let mut ctx = LaneContext::new();
///     ctx.insert_ref(&value);
///     escaped = ctx.get_ref::<u32>();
/// }
/// let _ = escaped.is_some();
/// ```
///
/// A lane cannot keep a borrow it was lent past its `execute`:
///
/// ```rust,compile_fail,E0521
/// use khora_core::lane::{Lane, LaneContext, LaneError, LaneKind};
/// use std::any::Any;
/// use std::sync::Mutex;
///
/// struct Hoarder(Mutex<Option<&'static u32>>);
///
/// impl Lane for Hoarder {
///     fn strategy_name(&self) -> &'static str { "hoarder" }
///     fn lane_kind(&self) -> LaneKind { LaneKind::Render }
///     fn execute(&self, ctx: &mut LaneContext) -> Result<(), LaneError> {
///         *self.0.lock().unwrap() = ctx.get_ref::<u32>();
///         Ok(())
///     }
///     fn as_any(&self) -> &dyn Any { self }
///     fn as_any_mut(&mut self) -> &mut dyn Any { self }
/// }
/// ```
#[cfg(doctest)]
struct SoundnessGuards;
