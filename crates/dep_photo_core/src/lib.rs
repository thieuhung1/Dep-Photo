pub const TILE_SIZE: usize = 256;
pub const CHANNELS: usize = 4;
pub const TILE_PIXELS: usize = TILE_SIZE * TILE_SIZE;
pub const TILE_BUFFER_LEN: usize = TILE_PIXELS * CHANNELS;

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

    /// Tạo tile đồng màu (ví dụ nền trắng, nền đen) với tốc độ tối đa
    pub fn new_solid(rgba: [f32; 4]) -> Self {
        let mut tile = Self::new();
        tile.fill(rgba);
        tile
    }

    /// Lấp đầy toàn bộ tile bằng 1 màu duy nhất
    pub fn fill(&mut self, rgba: [f32; 4]) {
        for chunk in self.data.chunks_exact_mut(CHANNELS) {
            chunk.copy_from_slice(&rgba);
        }
    }

    /// Kiểm tra nhanh tile có hoàn toàn trong suốt không (phục vụ skip-render ADR-002)
    pub fn is_transparent(&self) -> bool {
        self.data.chunks_exact(CHANNELS).all(|c| c[3] <= 0.001)
    }

    #[inline(always)]
    fn pixel_index(x: usize, y: usize) -> usize {
        debug_assert!(x < TILE_SIZE && y < TILE_SIZE);
        (y * TILE_SIZE + x) * CHANNELS
    }

    #[inline(always)]
    pub fn get_pixel(&self, x: usize, y: usize) -> [f32; 4] {
        let idx = Self::pixel_index(x, y);
        [
            self.data[idx],
            self.data[idx + 1],
            self.data[idx + 2],
            self.data[idx + 3],
        ]
    }

    #[inline(always)]
    pub fn set_pixel(&mut self, x: usize, y: usize, rgba: [f32; 4]) {
        let idx = Self::pixel_index(x, y);
        self.data[idx..idx + 4].copy_from_slice(&rgba);
    }
}

/// Hàm hòa trộn màu chuẩn Porter-Duff "Over" (Linear sRGB)
#[inline(always)]
pub fn blend_pixel_over(dst: &mut [f32; 4], src: [f32; 4], alpha_factor: f32) {
    let sa = src[3] * alpha_factor;
    if sa <= 0.001 {
        return;
    }
    let inv = 1.0 - sa;
    dst[0] = src[0] * sa + dst[0] * inv;
    dst[1] = src[1] * sa + dst[1] * inv;
    dst[2] = src[2] * sa + dst[2] * inv;
    dst[3] = sa + dst[3] * inv;
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

    #[test]
    fn test_tile_new_solid_and_fill() {
        let white = [1.0, 1.0, 1.0, 1.0];
        let tile = Tile::new_solid(white);
        assert_eq!(tile.get_pixel(0, 0), white);
        assert_eq!(tile.get_pixel(255, 255), white);
        assert!(!tile.is_transparent());

        let transparent = Tile::new();
        assert!(transparent.is_transparent());
    }

    #[test]
    fn test_blend_pixel_over() {
        let mut bg = [1.0, 1.0, 1.0, 1.0]; // Nền trắng
        let black = [0.0, 0.0, 0.0, 1.0];  // Màu đen đục
        blend_pixel_over(&mut bg, black, 0.5); // Đen 50% opacity đè lên trắng
        assert!((bg[0] - 0.5).abs() < 0.01);
        assert!((bg[1] - 0.5).abs() < 0.01);
        assert!((bg[2] - 0.5).abs() < 0.01);
        assert_eq!(bg[3], 1.0);
    }
}
