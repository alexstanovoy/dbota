//! The map's grid: the match's terrain cells, which the map from the cache is drawn over.

/// Terrain cells per axis of the map.
pub const MAP_CELLS: usize = bota_core::rules::GRID_CELLS;

/// World units of one terrain cell.
pub const CELL: f32 = bota_core::rules::GRID_CELL_SIZE as f32;
