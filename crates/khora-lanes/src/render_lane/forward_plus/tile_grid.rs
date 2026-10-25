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

//! The tile grid Forward+ culls lights into, derived from the colour target.

use khora_core::math::Extent2D;
use khora_core::renderer::ForwardPlusTileConfig;

/// The tiles Forward+ culls lights into for one frame: the colour target cut
/// into square tiles of the configured size (the last row/column may be partial).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TileGrid {
    /// The colour target the grid covers, in pixels.
    pub target: Extent2D,
    /// Tiles across the target's width.
    pub tiles_x: u32,
    /// Tiles across the target's height.
    pub tiles_y: u32,
    /// Side of one square tile, in pixels.
    pub tile_size: u32,
}

impl TileGrid {
    /// Cuts `target` into tiles of `config.tile_size`, rounding each side up.
    pub fn new(config: &ForwardPlusTileConfig, target: Extent2D) -> Self {
        let (tiles_x, tiles_y) = config.tile_dimensions(target.width, target.height);
        Self {
            target,
            tiles_x,
            tiles_y,
            tile_size: config.tile_size.pixels(),
        }
    }

    /// The number of tiles in the grid.
    pub fn total(&self) -> u32 {
        self.tiles_x * self.tiles_y
    }

    /// The `tile_info` uniform: `[tiles_x, tiles_y, tile_size, max_lights_per_tile]`.
    pub fn tile_info(&self, max_lights_per_tile: u32) -> [u32; 4] {
        [
            self.tiles_x,
            self.tiles_y,
            self.tile_size,
            max_lights_per_tile,
        ]
    }

    /// The target's `[width, height]` in pixels, as the culling shader reads it.
    pub fn screen_dimensions(&self) -> [f32; 2] {
        [self.target.width as f32, self.target.height as f32]
    }
}

/// How many tiles the light-list buffers must be regrown to hold, or `None`
/// when `capacity` already holds `needed`. Grow-only: a shrinking target keeps
/// its buffers, so a resize drag does not reallocate every frame.
pub(crate) fn grown_tile_capacity(capacity: u32, needed: u32) -> Option<u32> {
    (needed > capacity).then_some(needed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::renderer::TileSize;

    fn extent(width: u32, height: u32) -> Extent2D {
        Extent2D { width, height }
    }

    fn config(tile_size: TileSize, max_lights_per_tile: u32) -> ForwardPlusTileConfig {
        ForwardPlusTileConfig {
            tile_size,
            max_lights_per_tile,
            use_depth_prepass: false,
        }
    }

    fn x16() -> ForwardPlusTileConfig {
        config(TileSize::X16, 128)
    }

    #[test]
    fn a_1920_by_1080_target_at_16_pixels_is_120_by_68_tiles() {
        let grid = TileGrid::new(&x16(), extent(1920, 1080));
        assert_eq!((grid.tiles_x, grid.tiles_y), (120, 68));
        assert_eq!(grid.tile_size, 16);
        assert_eq!(grid.target, extent(1920, 1080));
        assert_eq!(grid.total(), 120 * 68);
    }

    #[test]
    fn a_1024_by_768_target_at_16_pixels_is_64_by_48_tiles() {
        let grid = TileGrid::new(&x16(), extent(1024, 768));
        assert_eq!((grid.tiles_x, grid.tiles_y), (64, 48));
        assert_eq!(grid.total(), 64 * 48);
    }

    #[test]
    fn a_partial_last_row_and_column_count_as_whole_tiles() {
        // 1384 / 16 = 86.5 and 561 / 16 = 35.06: both round up.
        let grid = TileGrid::new(&x16(), extent(1384, 561));
        assert_eq!((grid.tiles_x, grid.tiles_y), (87, 36));
        assert_eq!(grid.total(), 87 * 36);
    }

    #[test]
    fn the_configured_tile_size_cuts_the_target() {
        // 1384 / 32 = 43.25 and 561 / 32 = 17.53: both round up.
        let grid = TileGrid::new(&config(TileSize::X32, 128), extent(1384, 561));
        assert_eq!((grid.tiles_x, grid.tiles_y), (44, 18));
        assert_eq!(grid.tile_size, 32);
        assert_eq!(grid.total(), 44 * 18);
    }

    #[test]
    fn a_target_smaller_than_one_tile_is_one_tile() {
        let grid = TileGrid::new(&x16(), extent(10, 5));
        assert_eq!((grid.tiles_x, grid.tiles_y), (1, 1));
        assert_eq!(grid.total(), 1);
    }

    #[test]
    fn a_target_with_a_zero_side_has_no_tiles() {
        for target in [extent(0, 1080), extent(1920, 0), extent(0, 0)] {
            let grid = TileGrid::new(&x16(), target);
            assert_eq!(grid.total(), 0, "{target:?} has no area, so no tile");
            assert_eq!(grid.target, target);
        }
    }

    #[test]
    fn tile_info_carries_the_target_grid_and_the_light_budget() {
        let grid = TileGrid::new(&x16(), extent(1384, 561));
        assert_eq!(grid.tile_info(128), [87, 36, 16, 128]);

        let grid = TileGrid::new(&config(TileSize::X32, 256), extent(1024, 768));
        assert_eq!(grid.tile_info(256), [32, 24, 32, 256]);
    }

    #[test]
    fn screen_dimensions_are_the_target_not_a_fixed_1080p() {
        let grid = TileGrid::new(&x16(), extent(1384, 561));
        assert_eq!(grid.screen_dimensions(), [1384.0, 561.0]);

        let grid = TileGrid::new(&x16(), extent(1024, 768));
        assert_eq!(grid.screen_dimensions(), [1024.0, 768.0]);
    }

    #[test]
    fn capacity_grows_to_exactly_what_a_larger_grid_needs() {
        assert_eq!(grown_tile_capacity(0, 8160), Some(8160));
        assert_eq!(grown_tile_capacity(3072, 8160), Some(8160));
        assert_eq!(grown_tile_capacity(8159, 8160), Some(8160));
    }

    #[test]
    fn capacity_that_already_holds_the_grid_is_kept() {
        assert_eq!(grown_tile_capacity(8160, 8160), None);
        assert_eq!(grown_tile_capacity(8160, 3072), None);
        assert_eq!(grown_tile_capacity(8160, 1), None);
    }

    #[test]
    fn a_grid_without_tiles_never_asks_for_buffers() {
        assert_eq!(grown_tile_capacity(0, 0), None);
        assert_eq!(grown_tile_capacity(8160, 0), None);
    }
}
