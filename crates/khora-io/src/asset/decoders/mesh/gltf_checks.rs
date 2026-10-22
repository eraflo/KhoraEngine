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

//! The checks a glTF file must pass before its data is read.
//!
//! A glTF file is untrusted input. `gltf-json` validates that indices point
//! at existing objects, but not that an accessor's type fits the attribute
//! naming it, that its count is at least one, or that its bytes lie inside
//! its buffer; the `gltf` reader then reaches `unreachable!`, unchecked
//! arithmetic or `unwrap` on such a file. Every accessor and image the
//! importer reads is checked here first, with checked arithmetic, so a
//! hostile file is refused with an error instead of taking down whatever is
//! loading it.

use std::error::Error;

use gltf::accessor::{DataType, Dimensions};
use gltf::mesh::Semantic;

type BoxError = Box<dyn Error + Send + Sync>;

/// The most elements an accessor without a buffer view may describe. Such an
/// accessor is all zeros (plus sparse substitutions), so its count is not
/// bounded by any data in the file; without a cap a single number in the
/// JSON could ask for gigabytes. Real files use it for small zero-filled
/// attributes, far below this.
const MAX_UNBACKED_ELEMENTS: usize = 1 << 20;

/// The most vertices one decoded mesh may hold (768 MiB of 48-byte
/// vertices). A file that places one mesh under many nodes is decoded once
/// per placement when its scene is flattened, so a few bytes of JSON could
/// otherwise multiply a mesh past any memory.
pub(super) const MAX_DECODED_VERTICES: usize = 1 << 24;

/// Checks every accessor `primitive` gives the importer: `POSITION`,
/// `NORMAL`, `TEXCOORD_0`, `COLOR_0` and the indices. `buffers` are the
/// file's loaded buffers, in document order.
pub(super) fn check_primitive(
    primitive: &gltf::Primitive<'_>,
    buffers: &[Vec<u8>],
) -> Result<(), BoxError> {
    for (semantic, accessor) in primitive.attributes() {
        let allowed: &[(Dimensions, &[DataType])] = match semantic {
            Semantic::Positions | Semantic::Normals => &[(Dimensions::Vec3, &[DataType::F32])],
            Semantic::TexCoords(0) => &[(
                Dimensions::Vec2,
                &[DataType::F32, DataType::U8, DataType::U16],
            )],
            Semantic::Colors(0) => &[
                (
                    Dimensions::Vec3,
                    &[DataType::F32, DataType::U8, DataType::U16],
                ),
                (
                    Dimensions::Vec4,
                    &[DataType::F32, DataType::U8, DataType::U16],
                ),
            ],
            // Not read by the importer.
            _ => continue,
        };
        let fits = allowed.iter().any(|(dimensions, types)| {
            accessor.dimensions() == *dimensions && types.contains(&accessor.data_type())
        });
        if !fits {
            return Err(format!(
                "glTF accessor {} ({:?} {:?}) cannot hold {semantic:?}",
                accessor.index(),
                accessor.data_type(),
                accessor.dimensions()
            )
            .into());
        }
        check_accessor(&accessor, buffers)?;
    }
    if let Some(indices) = primitive.indices() {
        let fits = indices.dimensions() == Dimensions::Scalar
            && matches!(
                indices.data_type(),
                DataType::U8 | DataType::U16 | DataType::U32
            );
        if !fits {
            return Err(format!(
                "glTF accessor {} ({:?} {:?}) cannot hold indices",
                indices.index(),
                indices.data_type(),
                indices.dimensions()
            )
            .into());
        }
        check_accessor(&indices, buffers)?;
    }
    Ok(())
}

/// Checks that every byte `accessor` reads lies inside its buffers.
fn check_accessor(accessor: &gltf::Accessor<'_>, buffers: &[Vec<u8>]) -> Result<(), BoxError> {
    let index = accessor.index();
    let count = accessor.count();
    if count == 0 {
        return Err(format!("glTF accessor {index} has no elements").into());
    }
    let element = element_size(accessor.data_type(), accessor.dimensions());

    match accessor.view() {
        Some(view) => {
            let stride = match view.stride() {
                Some(stride) if stride < element => {
                    return Err(format!(
                        "glTF accessor {index}: stride {stride} is shorter than its {element}-byte \
                         elements"
                    )
                    .into());
                }
                Some(stride) => stride,
                None => element,
            };
            let end = (count - 1)
                .checked_mul(stride)
                .and_then(|span| span.checked_add(element))
                .and_then(|span| span.checked_add(accessor.offset()))
                .ok_or_else(|| format!("glTF accessor {index}: its extent overflows"))?;
            if end > view.length() {
                return Err(format!(
                    "glTF accessor {index} reads {end} bytes of a {}-byte buffer view",
                    view.length()
                )
                .into());
            }
            check_view(&view, buffers)?;
        }
        None if count > MAX_UNBACKED_ELEMENTS => {
            return Err(format!(
                "glTF accessor {index} has no buffer view but claims {count} elements \
                 (at most {MAX_UNBACKED_ELEMENTS})"
            )
            .into());
        }
        None => {}
    }

    if let Some(sparse) = accessor.sparse() {
        let substitutions = sparse.count();
        if substitutions == 0 || substitutions > count {
            return Err(format!(
                "glTF accessor {index}: {substitutions} sparse values for {count} elements"
            )
            .into());
        }
        let index_size = match sparse.indices().index_type() {
            gltf::accessor::sparse::IndexType::U8 => 1,
            gltf::accessor::sparse::IndexType::U16 => 2,
            gltf::accessor::sparse::IndexType::U32 => 4,
        };
        let indices = sparse.indices();
        let values = sparse.values();
        // glTF: the views of sparse data are tightly packed; the reader steps
        // by a declared stride all the same, so one would misread them.
        for (what, view) in [("indices", indices.view()), ("values", values.view())] {
            if view.stride().is_some() {
                return Err(format!(
                    "glTF accessor {index}: its sparse {what} view declares a byteStride"
                )
                .into());
            }
        }
        check_span(
            index,
            "sparse indices",
            indices.offset(),
            substitutions,
            index_size,
            &indices.view(),
        )?;
        check_span(
            index,
            "sparse values",
            values.offset(),
            substitutions,
            element,
            &values.view(),
        )?;
        check_view(&values.view(), buffers)?;
        let bytes = view_bytes(&indices.view(), buffers)?;
        let start = indices.offset();
        let bytes = &bytes[start..start + substitutions * index_size];
        check_sparse_indices(index, bytes, index_size, count)?;
    }
    Ok(())
}

/// glTF: sparse indices strictly increase and name elements of the accessor.
/// The reader matches each index against a running counter, so one out of
/// order or out of range would be dropped without a word, and the mesh would
/// show a shape the file does not describe.
fn check_sparse_indices(
    accessor: usize,
    bytes: &[u8],
    index_size: usize,
    count: usize,
) -> Result<(), BoxError> {
    let mut previous: Option<usize> = None;
    for chunk in bytes.chunks_exact(index_size) {
        let value = match *chunk {
            [a] => usize::from(a),
            [a, b] => usize::from(u16::from_le_bytes([a, b])),
            [a, b, c, d] => u32::from_le_bytes([a, b, c, d]) as usize,
            _ => unreachable!("index sizes are 1, 2 or 4"),
        };
        if value >= count || previous.is_some_and(|previous| value <= previous) {
            return Err(format!(
                "glTF accessor {accessor}: sparse indices must strictly increase below {count}, \
                 found {value} after {previous:?}"
            )
            .into());
        }
        previous = Some(value);
    }
    Ok(())
}

/// Checks that `count` tightly packed `size`-byte items from `offset` fit in
/// `view`.
fn check_span(
    accessor: usize,
    what: &str,
    offset: usize,
    count: usize,
    size: usize,
    view: &gltf::buffer::View<'_>,
) -> Result<(), BoxError> {
    let end = count
        .checked_mul(size)
        .and_then(|span| span.checked_add(offset))
        .ok_or_else(|| format!("glTF accessor {accessor}: its {what} overflow"))?;
    if end > view.length() {
        return Err(format!(
            "glTF accessor {accessor}: its {what} read {end} bytes of a {}-byte buffer view",
            view.length()
        )
        .into());
    }
    Ok(())
}

/// The bytes of `view` inside its loaded buffer, or why they are not there.
pub(super) fn view_bytes<'b>(
    view: &gltf::buffer::View<'_>,
    buffers: &'b [Vec<u8>],
) -> Result<&'b [u8], BoxError> {
    let buffer = buffers
        .get(view.buffer().index())
        .ok_or_else(|| format!("glTF buffer view {}: no such buffer", view.index()))?;
    view.offset()
        .checked_add(view.length())
        .and_then(|end| buffer.get(view.offset()..end))
        .ok_or_else(|| {
            format!(
                "glTF buffer view {} lies outside its {}-byte buffer",
                view.index(),
                buffer.len()
            )
            .into()
        })
}

/// Checks that `view` lies inside its loaded buffer.
fn check_view(view: &gltf::buffer::View<'_>, buffers: &[Vec<u8>]) -> Result<(), BoxError> {
    view_bytes(view, buffers).map(|_| ())
}

/// Checks that image `index` names bytes the `gltf` crate can hand over: a
/// buffer view with its media type, or a URI.
pub(super) fn check_image(document: &gltf::Document, index: usize) -> Result<(), BoxError> {
    let image = document
        .as_json()
        .images
        .get(index)
        .ok_or_else(|| format!("glTF image {index} does not exist"))?;
    match (&image.buffer_view, &image.mime_type, &image.uri) {
        (Some(_), Some(_), _) | (None, _, Some(_)) => Ok(()),
        (Some(_), None, _) => {
            Err(format!("glTF image {index} is in a buffer view but has no mimeType").into())
        }
        (None, _, None) => {
            Err(format!("glTF image {index} has neither a uri nor a buffer view").into())
        }
    }
}

/// The size in bytes of one element of `data_type` × `dimensions`, padding of
/// matrix columns aside (no attribute the importer reads is a matrix).
fn element_size(data_type: DataType, dimensions: Dimensions) -> usize {
    let component = match data_type {
        DataType::I8 | DataType::U8 => 1,
        DataType::I16 | DataType::U16 => 2,
        DataType::U32 | DataType::F32 => 4,
    };
    let components = match dimensions {
        Dimensions::Scalar => 1,
        Dimensions::Vec2 => 2,
        Dimensions::Vec3 => 3,
        Dimensions::Vec4 | Dimensions::Mat2 => 4,
        Dimensions::Mat3 => 9,
        Dimensions::Mat4 => 16,
    };
    component * components
}
