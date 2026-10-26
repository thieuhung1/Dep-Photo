pub const TILE_SIZE: usize = 256;
pub const CHANNELS: usize = 4;
pub const TILE_BUFFER_LEN: usize = TILE_SIZE * TILE_SIZE * CHANNELS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileCoord {
    pub x: i32,
    pub y: i32,
}

impl TileCoord {
    #[inline]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// Chuyển đổi tọa độ pixel document sang TileCoord
    #[inline]
    pub const fn from_pixel(px: i32, py: i32) -> Self {
        Self {
            x: px.div_euclid(TILE_SIZE as i32),
            y: py.div_euclid(TILE_SIZE as i32),
        }
    }
}

/// Tile 256x256 pixel RGBA f32 (Linear sRGB)
#[derive(Clone, Debug, PartialEq)]
pub struct Tile {
    // ponytail: Box<[f32]> heap-allocated tránh tràn stack (1MB/tile), nâng cấp sang pool allocator khi đạt >1000 tiles
    pub data: Box<[f32]>,
}

impl Default for Tile {
    fn default() -> Self {
        Self {
            data: vec![0.0f32; TILE_BUFFER_LEN].into_boxed_slice(),
        }
    }
}

impl Tile {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    fn pixel_index(x: usize, y: usize) -> usize {
        debug_assert!(x < TILE_SIZE && y < TILE_SIZE);
        (y * TILE_SIZE + x) * CHANNELS
    }

    #[inline]
    pub fn get_pixel(&self, x: usize, y: usize) -> [f32; 4] {
        let idx = Self::pixel_index(x, y);
        [
            self.data[idx],
            self.data[idx + 1],
            self.data[idx + 2],
            self.data[idx + 3],
        ]
    }

    #[inline]
    pub fn set_pixel(&mut self, x: usize, y: usize, rgba: [f32; 4]) {
        let idx = Self::pixel_index(x, y);
        self.data[idx..idx + 4].copy_from_slice(&rgba);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tile_coord_from_pixel() {
        assert_eq!(TileCoord::from_pixel(0, 0), TileCoord::new(0, 0));
        assert_eq!(TileCoord::from_pixel(255, 255), TileCoord::new(0, 0));
        assert_eq!(TileCoord::from_pixel(256, 0), TileCoord::new(1, 0));
        assert_eq!(TileCoord::from_pixel(-1, -1), TileCoord::new(-1, -1));
        assert_eq!(TileCoord::from_pixel(-256, -256), TileCoord::new(-1, -1));
        assert_eq!(TileCoord::from_pixel(-257, -1), TileCoord::new(-2, -1));
    }

    #[test]
    fn test_tile_pixel_read_write() {
        let mut tile = Tile::new();
        assert_eq!(tile.get_pixel(10, 20), [0.0, 0.0, 0.0, 0.0]);

        let red = [1.0, 0.0, 0.0, 1.0];
        tile.set_pixel(10, 20, red);
        assert_eq!(tile.get_pixel(10, 20), red);
    }
}
