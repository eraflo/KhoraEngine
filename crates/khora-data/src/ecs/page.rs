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

use std::{
    any::{Any, TypeId},
    collections::HashMap,
    fmt,
};

use bincode::{Decode, Encode};
use khora_core::ecs::entity::EntityId;

/// Upper bound on the byte payload accepted by [`AnyVec::set_from_bytes`] for a
/// single component column.
///
/// Deserialization feeds this length straight into a `Vec` reservation, so an
/// untrusted scene/pack file could otherwise request an arbitrarily large
/// allocation and abort the process with an OOM. Real component columns are
/// small (transforms, handles, a few scalars per entity); even a million
/// entities of a 256-byte component is 256 MiB, so this ceiling comfortably
/// covers legitimate scenes while rejecting hostile inputs before any
/// allocation happens.
pub const MAX_COLUMN_PAYLOAD_BYTES: usize = 256 * 1024 * 1024;

/// Error returned when raw column bytes fail validation in
/// [`AnyVec::set_from_bytes`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetFromBytesError {
    /// The byte length is not an exact multiple of the column's element size,
    /// so it cannot describe a whole number of elements.
    MisalignedLength {
        /// Length of the supplied byte buffer.
        len: usize,
        /// Size of a single element in the column.
        elem_size: usize,
    },
    /// The byte length exceeds [`MAX_COLUMN_PAYLOAD_BYTES`].
    PayloadTooLarge {
        /// Length of the supplied byte buffer.
        len: usize,
        /// The maximum accepted payload size.
        max: usize,
    },
}

impl fmt::Display for SetFromBytesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SetFromBytesError::MisalignedLength { len, elem_size } => write!(
                f,
                "column byte length {len} is not a multiple of element size {elem_size}"
            ),
            SetFromBytesError::PayloadTooLarge { len, max } => write!(
                f,
                "column byte length {len} exceeds the maximum payload size {max}"
            ),
        }
    }
}

impl std::error::Error for SetFromBytesError {}

/// An internal helper trait to perform vector operations on a type-erased `Box<dyn Any>`.
///
/// This allows us to call methods like `swap_remove` on component columns without
/// needing to know their concrete `Vec<T>` type at compile time.
pub trait AnyVec: Any + Send + Sync {
    /// Casts the trait object to `&dyn Any`.
    fn as_any(&self) -> &dyn Any;

    /// Casts the trait object to `&mut dyn Any`.
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Performs a `swap_remove` on the underlying column, removing the element at `index`.
    fn swap_remove_any(&mut self, index: usize);

    /// Serialises the column to an owned byte buffer.
    ///
    /// Returns an *owned* `Vec<u8>` (not a borrowed slice) so that columns whose
    /// data is not a single contiguous buffer — e.g. a field-split SoA column —
    /// can materialise their bytes. The format is the column's own concern; the
    /// only contract is that [`set_from_bytes`](AnyVec::set_from_bytes) on the
    /// same column type reverses it.
    fn to_bytes(&self) -> Vec<u8>;

    /// Replaces the column contents from bytes previously produced by
    /// [`to_bytes`](AnyVec::to_bytes) **on the same column type**.
    ///
    /// The length is validated against the column's element size and against
    /// [`MAX_COLUMN_PAYLOAD_BYTES`] before any allocation, so untrusted input
    /// cannot force an out-of-memory abort; a malformed length returns
    /// [`SetFromBytesError`] and leaves the column unchanged.
    ///
    /// # Safety
    /// The caller must guarantee the bytes match this column's element *type*,
    /// element size, and alignment. Length validation alone cannot detect a
    /// type mismatch, so feeding bytes produced by a different column type is
    /// still undefined behaviour.
    unsafe fn set_from_bytes(&mut self, bytes: &[u8]) -> Result<(), SetFromBytesError>;
}

// We implement this trait for any `Vec<T>` where T is `'static`.
impl<T: 'static + Send + Sync> AnyVec for Vec<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn swap_remove_any(&mut self, index: usize) {
        self.swap_remove(index);
    }

    fn to_bytes(&self) -> Vec<u8> {
        // SAFETY: reads `len * size_of::<T>()` initialised bytes from the Vec's
        // buffer and copies them into an owned `Vec<u8>`.
        unsafe {
            std::slice::from_raw_parts(
                self.as_ptr() as *const u8,
                self.len() * std::mem::size_of::<T>(),
            )
            .to_vec()
        }
    }

    unsafe fn set_from_bytes(&mut self, bytes: &[u8]) -> Result<(), SetFromBytesError> {
        let elem_size = std::mem::size_of::<T>();
        if elem_size == 0 {
            return Ok(()); // Correctly handle Zero-Sized Types.
        }

        // Validate the untrusted length *before* reserving, so a hostile scene
        // file cannot trigger a huge allocation or a partial copy.
        if !bytes.len().is_multiple_of(elem_size) {
            return Err(SetFromBytesError::MisalignedLength {
                len: bytes.len(),
                elem_size,
            });
        }
        if bytes.len() > MAX_COLUMN_PAYLOAD_BYTES {
            return Err(SetFromBytesError::PayloadTooLarge {
                len: bytes.len(),
                max: MAX_COLUMN_PAYLOAD_BYTES,
            });
        }

        // Calculate the new length and resize the Vec accordingly.
        let new_len = bytes.len() / elem_size;
        self.clear();
        self.reserve(new_len);

        // SAFETY: `reserve(new_len)` guaranteed capacity for `new_len` elements,
        // i.e. `new_len * elem_size == bytes.len()` initialised bytes (the length
        // was validated as an exact multiple above). The source and destination
        // are distinct, non-overlapping allocations, so `copy_nonoverlapping` of
        // exactly `bytes.len()` bytes is in-bounds; `set_len(new_len)` then marks
        // those bytes initialised. Element *type* correctness is the documented
        // obligation of the caller.
        let ptr = self.as_mut_ptr() as *mut u8;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        self.set_len(new_len);
        Ok(())
    }
}

/// A logical address pointing to an entity's component data within a specific `ComponentPage`.
///
/// This struct is the core of the relational aspect of our ECS. It decouples an entity's
/// identity from the physical storage of its data by acting as a coordinate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Encode, Decode)]
pub struct PageIndex {
    /// The unique identifier of the `ComponentPage` that stores the component data.
    pub page_id: u32,
    /// The index of the row within the page where this entity's components are stored.
    pub row_index: u32,
}

/// A serializable representation of a single `ComponentPage`.
#[derive(Encode, Decode)]
pub(crate) struct SerializedPage {
    /// The unique identifiers of this page.
    pub(crate) type_names: Vec<String>,
    /// The list of entities whose component data is stored in this page.
    pub(crate) entities: Vec<EntityId>,
    /// The actual serialized component data columns. Each column is a byte vector
    /// representing the serialized `Vec<T>` for a specific component
    pub(crate) columns: HashMap<String, Vec<u8>>,
}

/// A page of memory that stores the component data for multiple entities
/// in a Structure of Arrays (SoA) layout.
///
/// A `ComponentPage` is specialized for a single semantic domain (e.g., physics).
/// It contains multiple columns, where each column is a `Vec<T>` for a specific
/// component type `T`. This SoA layout is the key to our high iteration performance,
/// as it guarantees contiguous data access for native queries.
pub struct ComponentPage {
    /// A map from a component's `TypeId` to its actual storage column.
    /// The `Box<dyn AnyVec>` is a type-erased `Vec<T>` that knows how to
    /// perform basic vector operations like `swap_remove`.
    pub(crate) columns: HashMap<TypeId, Box<dyn AnyVec>>,

    /// A list of the `EntityId`s that own the data in each row of this page.
    /// The entity at `entities[i]` corresponds to the components at `columns[...][i]`.
    /// This is crucial for reverse lookups, especially during entity despawning.
    pub(crate) entities: Vec<EntityId>,

    /// The sorted list of `TypeId`s for the components stored in this page.
    /// This acts as the page's "signature" for matching with bundles. It is
    /// kept sorted to ensure that the signature is canonical.
    pub(crate) type_ids: Vec<TypeId>,
}

impl ComponentPage {
    /// Adds an entity to this page's entity list.
    ///
    /// This method is called by `World::spawn` and is a crucial part of maintaining
    /// the invariant that the number of rows in the component columns is always
    /// equal to the number of entities tracked by the page.
    pub(crate) fn add_entity(&mut self, entity_id: EntityId) {
        self.entities.push(entity_id);
    }

    /// Performs a `swap_remove` on a specific row across all component columns
    /// and the entity list.
    ///
    /// This is the core of an O(1) despawn operation. It removes the data for the entity
    /// at `row_index` by swapping it with the last element in each column and in the
    /// entity list.
    ///
    /// It's the caller's (`World::despawn`) responsibility to update the metadata of
    /// the entity that was moved from the last row.
    pub(crate) fn swap_remove_row(&mut self, row_index: u32) {
        // 1. Remove the corresponding entity ID from the list. `swap_remove` on a Vec
        // returns the element that was at that index, but we don't need it here.
        self.entities.swap_remove(row_index as usize);

        // 2. Iterate through all component columns and perform the same swap_remove
        // on each one, using our `AnyVec` trait.
        for column in self.columns.values_mut() {
            column.swap_remove_any(row_index as usize);
        }
    }

    /// Returns the number of rows of data (and entities) this page currently stores.
    pub(crate) fn row_count(&self) -> usize {
        self.entities.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_from_bytes_valid_roundtrip() {
        let original: Vec<u32> = vec![1, 2, 3, 4];
        let bytes = original.to_bytes();
        let mut restored: Vec<u32> = Vec::new();
        // SAFETY: `bytes` was produced by `to_bytes` on a `Vec<u32>`, matching
        // the element type, size, and alignment of `restored`.
        unsafe { restored.set_from_bytes(&bytes) }.expect("valid bytes must round-trip");
        assert_eq!(restored, original);
    }

    #[test]
    fn set_from_bytes_misaligned_length_errs() {
        // 5 bytes is not a multiple of size_of::<u32>() == 4.
        let bytes = [0u8; 5];
        let mut col: Vec<u32> = vec![42];
        // SAFETY: the call validates length before touching memory; we only
        // assert it rejects a misaligned buffer.
        let result = unsafe { col.set_from_bytes(&bytes) };
        assert!(matches!(
            result,
            Err(SetFromBytesError::MisalignedLength {
                len: 5,
                elem_size: 4
            })
        ));
        // The column must be left untouched on error.
        assert_eq!(col, vec![42]);
    }

    #[test]
    fn set_from_bytes_oversized_payload_errs() {
        // A length that is a valid multiple of the element size but exceeds the
        // ceiling. We build a fake slice header without allocating the bytes.
        let elem_size = std::mem::size_of::<u8>();
        let oversized_len = MAX_COLUMN_PAYLOAD_BYTES + elem_size;
        // SAFETY: we never read through this pointer — `set_from_bytes`
        // validates the length and returns `Err` before any access. The dangling
        // pointer is only used to construct a slice whose length triggers the
        // ceiling check.
        let fake = unsafe {
            std::slice::from_raw_parts(std::ptr::NonNull::<u8>::dangling().as_ptr(), oversized_len)
        };
        let mut col: Vec<u8> = Vec::new();
        // SAFETY: as above — the oversized length short-circuits with an error
        // before the slice contents are ever touched.
        let result = unsafe { col.set_from_bytes(fake) };
        assert!(matches!(
            result,
            Err(SetFromBytesError::PayloadTooLarge { .. })
        ));
    }

    #[test]
    fn set_from_bytes_zero_sized_type_is_noop() {
        let mut col: Vec<()> = Vec::new();
        // SAFETY: ZST columns carry no byte payload; the call returns early.
        let result = unsafe { col.set_from_bytes(&[]) };
        assert!(result.is_ok());
    }
}
