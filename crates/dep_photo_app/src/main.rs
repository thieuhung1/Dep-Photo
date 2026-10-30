use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use dep_photo_core::{Tile, TileCoord, TILE_SIZE};
use eframe::egui::{
    self, Color32, ColorImage, CornerRadius, Pos2, Rect, Stroke, TextureOptions, Vec2,
};

// ─── Nén tile lịch sử (ADR-003 / zstd) ───────────────────────────
/// Tile đã nén bằng zstd — giảm ~80–90% RAM so với Box<[f32]> thô.
#[derive(Clone)]
struct CompressedTile {
    bytes: Vec<u8>,
    /// Số phần tử f32 khi giải nén (để cấp phát đúng kích thước).
    float_len: usize,
}

impl CompressedTile {
    fn compress(data: &[f32]) -> Self {
        let raw = f32_slice_to_bytes(data);
        // level 3: cân bằng tốc độ nén / tỷ lệ — đủ nhanh cho mỗi nét cọ
        let bytes = zstd::bulk::compress(&raw, 3).unwrap_or_else(|_| raw.clone());
        Self {
            bytes,
            float_len: data.len(),
        }
    }

    fn decompress(&self) -> Box<[f32]> {
        let expected_bytes = self.float_len * std::mem::size_of::<f32>();
        match zstd::bulk::decompress(&self.bytes, expected_bytes) {
            Ok(raw) if raw.len() == expected_bytes => bytes_to_f32_box(&raw),
            _ => {
                // Fallback: dữ liệu chưa nén hoặc hỏng → zero tile
                vec![0.0f32; self.float_len].into_boxed_slice()
            }
        }
    }

    fn compressed_bytes(&self) -> usize {
        self.bytes.len()
    }
}

#[inline]
fn f32_slice_to_bytes(data: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 4);
    for &v in data {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

#[inline]
fn bytes_to_f32_box(raw: &[u8]) -> Box<[f32]> {
    let n = raw.len() / 4;
    let mut out = Vec::with_capacity(n);
    for chunk in raw.chunks_exact(4) {
        out.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    out.into_boxed_slice()
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1366.0, 768.0])
            .with_min_inner_size([960.0, 600.0])
            .with_title("DEP_PHOTO — Trình chỉnh sửa ảnh hiệu năng cao"),
        ..Default::default()
    };
    eframe::run_native(
        "DEP_PHOTO",
        options,
        Box::new(|cc| {
            // Nạp font hệ thống Windows hỗ trợ đầy đủ tiếng Việt có dấu (Segoe UI / Arial / Tahoma)
            let mut fonts = egui::FontDefinitions::default();
            let font_candidates = [
                "C:\\Windows\\Fonts\\segoeui.ttf",
                "C:\\Windows\\Fonts\\arial.ttf",
                "C:\\Windows\\Fonts\\tahoma.ttf",
            ];
            for path in font_candidates {
                if let Ok(bytes) = std::fs::read(path) {
                    fonts.font_data.insert(
                        "system_vn_font".to_owned(),
                        Arc::new(egui::FontData::from_owned(bytes)),
                    );
                    fonts
                        .families
                        .entry(egui::FontFamily::Proportional)
                        .or_default()
                        .insert(0, "system_vn_font".to_owned());
                    fonts
                        .families
                        .entry(egui::FontFamily::Monospace)
                        .or_default()
                        .push("system_vn_font".to_owned());
                    break;
                }
            }
            cc.egui_ctx.set_fonts(fonts);

            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill = Color32::from_rgb(26, 26, 28);
            visuals.window_fill = Color32::from_rgb(30, 30, 34);
            visuals.extreme_bg_color = Color32::from_rgb(20, 20, 22);
            visuals.faint_bg_color = Color32::from_rgb(38, 38, 42);

            visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(30, 30, 34);
            visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(46, 46, 52));
            visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(200, 200, 210));
            visuals.widgets.noninteractive.corner_radius = CornerRadius::same(3);

            visuals.widgets.inactive.bg_fill = Color32::from_rgb(36, 36, 40);
            visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(50, 50, 58));
            visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(175, 175, 185));
            visuals.widgets.inactive.corner_radius = CornerRadius::same(3);

            visuals.widgets.hovered.bg_fill = Color32::from_rgb(48, 48, 56);
            visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(0, 162, 237));
            visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
            visuals.widgets.hovered.corner_radius = CornerRadius::same(3);

            visuals.widgets.active.bg_fill = Color32::from_rgb(0, 140, 215);
            visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(0, 180, 255));
            visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
            visuals.widgets.active.corner_radius = CornerRadius::same(3);

            let mut style = (*cc.egui_ctx.style()).clone();
            style.visuals = visuals;
            style.spacing.item_spacing = Vec2::new(6.0, 6.0);
            style.spacing.button_padding = Vec2::new(6.0, 4.0);
            cc.egui_ctx.set_style(style);

            Ok(Box::new(DepPhotoApp::new()))
        }),
    )
}

// ─── Danh sách công cụ (Tools) ──────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tool {
    Move,
    Marquee,
    Lasso,
    Crop,
    Eyedropper,
    Brush,
    Eraser,
    Bucket,
    Hand,
    Zoom,
    Text,
}

impl Tool {
    fn label(&self) -> &'static str {
        match self {
            Tool::Move => "Di chuyển lớp (V)",
            Tool::Marquee => "Chọn vùng chữ nhật (M)",
            Tool::Lasso => "Vùng chọn tự do Lasso (L)",
            Tool::Crop => "Cắt xén khung hình (C)",
            Tool::Eyedropper => "Lấy mẫu màu Eyedropper (I)",
            Tool::Brush => "Bút vẽ Brush (B)",
            Tool::Eraser => "Cục tẩy Eraser (E)",
            Tool::Bucket => "Đổ màu Paint Bucket (G)",
            Tool::Hand => "Bàn tay cuộn Hand (H)",
            Tool::Zoom => "Kính lúp Zoom (Z)",
            Tool::Text => "Chèn văn bản (T)",
        }
    }

    fn short_name(&self) -> &'static str {
        match self {
            Tool::Move => "Di chuyển (V)",
            Tool::Marquee => "Vùng chọn (M)",
            Tool::Lasso => "Lasso (L)",
            Tool::Crop => "Cắt xén (C)",
            Tool::Eyedropper => "Hút màu (I)",
            Tool::Brush => "Bút vẽ (B)",
            Tool::Eraser => "Cục tẩy (E)",
            Tool::Bucket => "Đổ màu (G)",
            Tool::Hand => "Bàn tay (H)",
            Tool::Zoom => "Thu phóng (Z)",
            Tool::Text => "Văn bản (T)",
        }
    }
}

const ALL_TOOLS: [Tool; 11] = [
    Tool::Move,
    Tool::Marquee,
    Tool::Lasso,
    Tool::Crop,
    Tool::Eyedropper,
    Tool::Brush,
    Tool::Eraser,
    Tool::Bucket,
    Tool::Hand,
    Tool::Zoom,
    Tool::Text,
];

// ─── Lớp ảnh (Layer) ─────────────────────────────────────────────
#[derive(Clone, Debug)]
struct Layer {
    name: String,
    visible: bool,
    locked: bool,
    opacity: f32,
    tiles: HashMap<TileCoord, Tile>,
}

impl Layer {
    fn new_white(name: &str, tiles_x: i32, tiles_y: i32, locked: bool) -> Self {
        let template = Tile::new_solid([1.0, 1.0, 1.0, 1.0]);
        let capacity = (tiles_x * tiles_y).max(0) as usize;
        let mut tiles = HashMap::with_capacity(capacity);
        for ty in 0..tiles_y {
            for tx in 0..tiles_x {
                tiles.insert(TileCoord::new(tx, ty), template.clone());
            }
        }
        Self {
            name: name.to_string(),
            visible: true,
            locked,
            opacity: 1.0,
            tiles,
        }
    }

    fn new_transparent(name: &str, tiles_x: i32, tiles_y: i32) -> Self {
        let capacity = (tiles_x * tiles_y).max(0) as usize;
        let mut tiles = HashMap::with_capacity(capacity);
        for ty in 0..tiles_y {
            for tx in 0..tiles_x {
                tiles.insert(TileCoord::new(tx, ty), Tile::new());
            }
        }
        Self {
            name: name.to_string(),
            visible: true,
            locked: false,
            opacity: 1.0,
            tiles,
        }
    }
}

// ─── Undo Step (Tile-Diff ADR-003, nén zstd) ─────────────────────
struct UndoStep {
    name: String,
    layer_idx: usize,
    /// Tile trước khi sửa, đã nén zstd.
    tiles: HashMap<TileCoord, CompressedTile>,
}

#[derive(Clone, Copy)]
struct NewCanvasDialog {
    width: u32,
    height: u32,
}

// ─── Trạng thái ứng dụng (App State) ────────────────────────────
struct DepPhotoApp {
    doc_width_px: usize,
    doc_height_px: usize,
    doc_tiles_x: i32,
    doc_tiles_y: i32,

    layers: Vec<Layer>,
    active_layer: usize,

    doc_texture: Option<egui::TextureHandle>,
    texture_dirty: bool,

    /// Cache CPU ảnh đã composite — dirty-tile blit chỉ ghi đè vùng thay đổi.
    composite_cache: Vec<Color32>,
    /// Các tile (tx, ty) cần composite lại. Rỗng + composite_full=false → không làm gì.
    composite_dirty: HashSet<(i32, i32)>,
    /// true = phải composite toàn bộ canvas (mở file, đổi layer, resize…).
    composite_full: bool,

    zoom: f32,
    pan: Vec2,
    show_grid: bool,

    current_tool: Tool,
    fg_color: [f32; 4],
    bg_color: [f32; 4],
    brush_radius: f32,
    brush_opacity: f32,
    brush_hardness: f32,

    last_stroke_pos: Option<Pos2>,
    /// Snapshot tile gốc (chưa nén) trong lúc đang kéo nét — nén khi commit undo.
    stroke_dirty_tiles: HashMap<TileCoord, Box<[f32]>>,
    /// Tile bị cọ chạm trong nét hiện tại (dùng cho dirty blit).
    stroke_touched_tiles: HashSet<(i32, i32)>,

    undo_stack: Vec<UndoStep>,
    redo_stack: Vec<UndoStep>,

    show_color_palette_window: bool,
    show_tips_window: bool,
    hsv_hue: f32,
    hsv_sat: f32,
    hsv_val: f32,

    mouse_doc_pos: Option<Pos2>,
    file_path: Option<PathBuf>,

    new_canvas_dialog: Option<NewCanvasDialog>,
    status_message: Option<(String, std::time::Instant)>,
}

impl DepPhotoApp {
    fn new() -> Self {
        let doc_width_px = 768;
        let doc_height_px = 768;
        let doc_tiles_x = 3;
        let doc_tiles_y = 3;

        let layers = vec![
            Layer::new_white("Lớp nền (Background)", doc_tiles_x, doc_tiles_y, true),
            Layer::new_transparent("Lớp vẽ 1", doc_tiles_x, doc_tiles_y),
        ];

        Self {
            doc_width_px,
            doc_height_px,
            doc_tiles_x,
            doc_tiles_y,
            layers,
            active_layer: 1,
            doc_texture: None,
            texture_dirty: true,
            composite_cache: Vec::new(),
            composite_dirty: HashSet::new(),
            composite_full: true,
            zoom: 1.0,
            pan: Vec2::ZERO,
            show_grid: true,
            current_tool: Tool::Brush,
            fg_color: [0.0, 0.58, 0.88, 1.0], // Deep cyan
            bg_color: [1.0, 1.0, 1.0, 1.0],   // White
            brush_radius: 12.0,
            brush_opacity: 1.0,
            brush_hardness: 0.85,
            last_stroke_pos: None,
            stroke_dirty_tiles: HashMap::new(),
            stroke_touched_tiles: HashSet::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            show_color_palette_window: true,
            show_tips_window: false,
            hsv_hue: 0.55,
            hsv_sat: 0.85,
            hsv_val: 0.95,
            mouse_doc_pos: None,
            file_path: None,
            new_canvas_dialog: None,
            status_message: None,
        }
    }

    /// Đánh dấu cần composite toàn bộ (đổi cấu trúc layer / kích thước canvas).
    fn mark_full_composite(&mut self) {
        self.composite_full = true;
        self.composite_dirty.clear();
        self.texture_dirty = true;
    }

    /// Đánh dấu một tile cần composite lại (dirty blit).
    fn mark_tile_dirty(&mut self, tx: i32, ty: i32) {
        if !self.composite_full {
            self.composite_dirty.insert((tx, ty));
        }
        self.texture_dirty = true;
    }

    #[inline]
    fn doc_width(&self) -> usize {
        self.doc_width_px
    }

    #[inline]
    fn doc_height(&self) -> usize {
        self.doc_height_px
    }

    fn total_memory_mb(&self) -> f32 {
        // Layer tiles (f32 thô) + undo/redo đã nén zstd + composite cache
        let bytes_per_tile = (TILE_SIZE * TILE_SIZE * 4 * 4) as f32;
        let mut layer_tiles = 0usize;
        for l in &self.layers {
            layer_tiles += l.tiles.len();
        }
        let mut undo_bytes = 0usize;
        for step in self.undo_stack.iter().chain(self.redo_stack.iter()) {
            for t in step.tiles.values() {
                undo_bytes += t.compressed_bytes();
            }
        }
        let cache_bytes = self.composite_cache.len() * 4; // Color32 ≈ 4 bytes
        (layer_tiles as f32 * bytes_per_tile + undo_bytes as f32 + cache_bytes as f32)
            / (1024.0 * 1024.0)
    }

    fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some((msg.into(), std::time::Instant::now()));
    }

    // ─── Quản lý Layer ──────────────────────────────────────────
    fn add_new_layer(&mut self) {
        let name = format!("Lớp vẽ {}", self.layers.len() + 1);
        let new_layer = Layer::new_transparent(&name, self.doc_tiles_x, self.doc_tiles_y);
        let insert_idx = (self.active_layer + 1).min(self.layers.len());
        self.layers.insert(insert_idx, new_layer);
        self.active_layer = insert_idx;
        self.mark_full_composite();
        self.set_status(format!("Đã tạo: {name}"));
    }

    fn duplicate_active_layer(&mut self) {
        if self.active_layer < self.layers.len() {
            let mut dup = self.layers[self.active_layer].clone();
            dup.name = format!("{} (Bản sao)", dup.name);
            dup.locked = false;
            let insert_idx = (self.active_layer + 1).min(self.layers.len());
            self.layers.insert(insert_idx, dup);
            self.active_layer = insert_idx;
            self.mark_full_composite();
            self.set_status("Đã nhân bản lớp ảnh");
        }
    }

    fn delete_active_layer(&mut self) {
        if self.layers.len() > 1 && self.active_layer < self.layers.len() {
            let removed = self.layers.remove(self.active_layer);
            if self.active_layer >= self.layers.len() {
                self.active_layer = self.layers.len() - 1;
            }
            self.mark_full_composite();
            self.set_status(format!("Đã xóa: {}", removed.name));
        }
    }

    fn move_active_layer_up(&mut self) {
        if self.active_layer + 1 < self.layers.len() {
            self.layers.swap(self.active_layer, self.active_layer + 1);
            self.active_layer += 1;
            self.mark_full_composite();
        }
    }

    fn move_active_layer_down(&mut self) {
        if self.active_layer > 0 {
            self.layers.swap(self.active_layer, self.active_layer - 1);
            self.active_layer -= 1;
            self.mark_full_composite();
        }
    }

    // ─── Tổng hợp ảnh đa lớp (Composite Engine + Dirty Tile Blit) ─
    /// Composite toàn bộ → buffer f32 (lưu file).
    fn composite_pixels(&self) -> Vec<[f32; 4]> {
        let w = self.doc_width();
        let h = self.doc_height();
        let mut pixels = vec![[0.0f32; 4]; w * h];
        self.composite_into_f32(&mut pixels, w, h);
        pixels
    }

    fn composite_into_f32(&self, pixels: &mut [[f32; 4]], w: usize, h: usize) {
        for layer in &self.layers {
            if !layer.visible || layer.opacity <= 0.0 {
                continue;
            }
            let layer_alpha = layer.opacity;
            for ty in 0..self.doc_tiles_y {
                for tx in 0..self.doc_tiles_x {
                    self.blend_tile_f32(layer, layer_alpha, tx, ty, pixels, w, h);
                }
            }
        }
    }

    /// Blend 1 tile của 1 layer vào buffer f32.
    fn blend_tile_f32(
        &self,
        layer: &Layer,
        layer_alpha: f32,
        tx: i32,
        ty: i32,
        pixels: &mut [[f32; 4]],
        w: usize,
        h: usize,
    ) {
        let Some(tile) = layer.tiles.get(&TileCoord::new(tx, ty)) else {
            return;
        };
        let ox = (tx as usize) * TILE_SIZE;
        let oy = (ty as usize) * TILE_SIZE;
        if ox >= w || oy >= h {
            return;
        }
        let max_y = TILE_SIZE.min(h - oy);
        let max_x = TILE_SIZE.min(w - ox);
        for y in 0..max_y {
            let row = (oy + y) * w + ox;
            for x in 0..max_x {
                let [sr, sg, sb, sa] = tile.get_pixel(x, y);
                let sa = sa * layer_alpha;
                if sa <= 0.001 {
                    continue;
                }
                let dst = &mut pixels[row + x];
                let inv = 1.0 - sa;
                dst[0] = sr * sa + dst[0] * inv;
                dst[1] = sg * sa + dst[1] * inv;
                dst[2] = sb * sa + dst[2] * inv;
                dst[3] = sa + dst[3] * inv;
            }
        }
    }

    /// Composite **một tile** (tất cả layer) → vùng tương ứng trong cache Color32.
    fn composite_one_tile_to_cache(&self, tx: i32, ty: i32, cache: &mut [Color32], w: usize, h: usize) {
        let ox = (tx as usize) * TILE_SIZE;
        let oy = (ty as usize) * TILE_SIZE;
        if ox >= w || oy >= h {
            return;
        }
        let max_y = TILE_SIZE.min(h - oy);
        let max_x = TILE_SIZE.min(w - ox);

        // Buffer tạm chỉ bằng 1 tile — không cấp phát full canvas
        let mut acc = vec![[0.0f32; 4]; max_x * max_y];

        for layer in &self.layers {
            if !layer.visible || layer.opacity <= 0.0 {
                continue;
            }
            let layer_alpha = layer.opacity;
            let Some(tile) = layer.tiles.get(&TileCoord::new(tx, ty)) else {
                continue;
            };
            for y in 0..max_y {
                for x in 0..max_x {
                    let [sr, sg, sb, sa] = tile.get_pixel(x, y);
                    let sa = sa * layer_alpha;
                    if sa <= 0.001 {
                        continue;
                    }
                    let dst = &mut acc[y * max_x + x];
                    let inv = 1.0 - sa;
                    dst[0] = sr * sa + dst[0] * inv;
                    dst[1] = sg * sa + dst[1] * inv;
                    dst[2] = sb * sa + dst[2] * inv;
                    dst[3] = sa + dst[3] * inv;
                }
            }
        }

        for y in 0..max_y {
            let row = (oy + y) * w + ox;
            for x in 0..max_x {
                let [r, g, b, a] = acc[y * max_x + x];
                cache[row + x] = Color32::from_rgba_unmultiplied(
                    (r.clamp(0.0, 1.0) * 255.0) as u8,
                    (g.clamp(0.0, 1.0) * 255.0) as u8,
                    (b.clamp(0.0, 1.0) * 255.0) as u8,
                    (a.clamp(0.0, 1.0) * 255.0) as u8,
                );
            }
        }
    }

    /// Full composite → cache Color32.
    fn composite_full_to_cache(&self, cache: &mut [Color32], w: usize, h: usize) {
        cache.fill(Color32::TRANSPARENT);
        let mut acc = vec![[0.0f32; 4]; w * h];
        self.composite_into_f32(&mut acc, w, h);
        for (dst, [r, g, b, a]) in cache.iter_mut().zip(acc.iter()) {
            *dst = Color32::from_rgba_unmultiplied(
                (r.clamp(0.0, 1.0) * 255.0) as u8,
                (g.clamp(0.0, 1.0) * 255.0) as u8,
                (b.clamp(0.0, 1.0) * 255.0) as u8,
                (a.clamp(0.0, 1.0) * 255.0) as u8,
            );
        }
    }

    /// Dirty Tile Blit: chỉ composite tile bẩn, giữ nguyên vùng còn lại trong cache.
    fn update_texture(&mut self, ctx: &egui::Context) {
        let w = self.doc_width();
        let h = self.doc_height();
        let needed = w * h;

        if self.composite_cache.len() != needed {
            self.composite_cache = vec![Color32::TRANSPARENT; needed];
            self.composite_full = true;
        }

        let mut cache = std::mem::take(&mut self.composite_cache);

        if self.composite_full {
            self.composite_full_to_cache(&mut cache, w, h);
            self.composite_dirty.clear();
            self.composite_full = false;
        } else if !self.composite_dirty.is_empty() {
            let dirty: Vec<(i32, i32)> = self.composite_dirty.drain().collect();
            for (tx, ty) in dirty {
                self.composite_one_tile_to_cache(tx, ty, &mut cache, w, h);
            }
        }

        self.composite_cache = cache;

        // Upload cache lên GPU (egui TextureHandle::set là full-image;
        // chi phí CPU composite đã giảm mạnh nhờ dirty blit).
        let image = ColorImage {
            size: [w, h],
            pixels: self.composite_cache.clone(),
        };
        match &mut self.doc_texture {
            Some(tex) => tex.set(image, TextureOptions::NEAREST),
            None => {
                self.doc_texture =
                    Some(ctx.load_texture("dep_canvas", image, TextureOptions::NEAREST));
            }
        }
        self.texture_dirty = false;
    }

    fn composite_image(&self) -> image::RgbaImage {
        let w = self.doc_width();
        let h = self.doc_height();
        let pixels = self.composite_pixels();

        let mut raw_bytes = Vec::with_capacity(w * h * 4);
        for [r, g, b, a] in pixels {
            raw_bytes.push((r.clamp(0.0, 1.0) * 255.0) as u8);
            raw_bytes.push((g.clamp(0.0, 1.0) * 255.0) as u8);
            raw_bytes.push((b.clamp(0.0, 1.0) * 255.0) as u8);
            raw_bytes.push((a.clamp(0.0, 1.0) * 255.0) as u8);
        }

        image::RgbaImage::from_raw(w as u32, h as u32, raw_bytes)
            .unwrap_or_else(|| image::RgbaImage::new(w as u32, h as u32))
    }

    // ─── Lưu & Mở tệp (File I/O) ────────────────────────────────
    fn open_image_file(&mut self, path: PathBuf) {
        if let Ok(img) = image::open(&path) {
            let rgba = img.to_rgba8();
            let (iw, ih) = rgba.dimensions();
            let w = iw as usize;
            let h = ih as usize;
            let tx = ((w + TILE_SIZE - 1) / TILE_SIZE) as i32;
            let ty = ((h + TILE_SIZE - 1) / TILE_SIZE) as i32;

            self.doc_width_px = w;
            self.doc_height_px = h;
            self.doc_tiles_x = tx;
            self.doc_tiles_y = ty;

            // Tạo layer trống rồi fill theo tile — không double-pass (white rồi overwrite)
            let mut bg = Layer::new_transparent("Lớp nền (Gốc)", tx, ty);
            bg.locked = true;

            let tile_size_i = TILE_SIZE as i32;
            for ty_i in 0..ty {
                for tx_i in 0..tx {
                    let coord = TileCoord::new(tx_i, ty_i);
                    let Some(tile) = bg.tiles.get_mut(&coord) else {
                        continue;
                    };
                    let base_x = tx_i * tile_size_i;
                    let base_y = ty_i * tile_size_i;
                    let max_lx = ((iw as i32 - base_x).max(0) as usize).min(TILE_SIZE);
                    let max_ly = ((ih as i32 - base_y).max(0) as usize).min(TILE_SIZE);

                    for ly in 0..max_ly {
                        let py = (base_y + ly as i32) as u32;
                        for lx in 0..max_lx {
                            let px = (base_x + lx as i32) as u32;
                            let p = rgba.get_pixel(px, py);
                            tile.set_pixel(
                                lx,
                                ly,
                                [
                                    p[0] as f32 / 255.0,
                                    p[1] as f32 / 255.0,
                                    p[2] as f32 / 255.0,
                                    p[3] as f32 / 255.0,
                                ],
                            );
                        }
                    }
                }
            }

            let file_name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            self.layers = vec![bg, Layer::new_transparent("Lớp vẽ 1", tx, ty)];
            self.active_layer = 1;
            self.undo_stack.clear();
            self.redo_stack.clear();
            self.stroke_dirty_tiles.clear();
            self.stroke_touched_tiles.clear();
            self.file_path = Some(path);
            self.pan = Vec2::ZERO;
            self.zoom = 1.0;
            self.composite_cache.clear(); // force resize in update_texture
            self.mark_full_composite();
            self.set_status(format!("Đã mở ảnh: {file_name} ({w}×{h} px)"));
        } else {
            self.set_status("Không thể mở tập tin ảnh!");
        }
    }

    fn save_file_as(&mut self, default_format: Option<&str>) {
        let mut dialog = rfd::FileDialog::new()
            .add_filter("Tất cả định dạng ảnh", &["png", "jpg", "jpeg", "webp"])
            .add_filter("Định dạng PNG (*.png)", &["png"])
            .add_filter("Định dạng JPEG (*.jpg, *.jpeg)", &["jpg", "jpeg"])
            .add_filter("Định dạng WebP (*.webp)", &["webp"]);

        if let Some(path) = &self.file_path {
            if let Some(parent) = path.parent() {
                dialog = dialog.set_directory(parent);
            }
            if let Some(name) = path.file_name() {
                dialog = dialog.set_file_name(name.to_string_lossy());
            }
        } else {
            let default_name = format!("tac_pham.{}", default_format.unwrap_or("png"));
            dialog = dialog.set_file_name(default_name);
        }

        if let Some(mut path) = dialog.save_file() {
            if path.extension().is_none() {
                path.set_extension(default_format.unwrap_or("png"));
            }

            let img = self.composite_image();
            match img.save(&path) {
                Ok(_) => {
                    let file_name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                    self.set_status(format!("Đã lưu thành công: {file_name}"));
                    self.file_path = Some(path);
                }
                Err(err) => {
                    self.set_status(format!("Lỗi khi lưu ảnh: {err}"));
                }
            }
        }
    }

    fn save_file(&mut self) {
        if let Some(path) = self.file_path.clone() {
            let img = self.composite_image();
            match img.save(&path) {
                Ok(_) => {
                    let file_name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                    self.set_status(format!("Đã lưu thành công: {file_name}"));
                }
                Err(err) => {
                    self.set_status(format!("Lỗi khi lưu: {err}"));
                }
            }
        } else {
            self.save_file_as(Some("png"));
        }
    }

    fn create_new_canvas(&mut self, width: u32, height: u32) {
        let w = width.clamp(64, 8192) as usize;
        let h = height.clamp(64, 8192) as usize;
        let tx = ((w + TILE_SIZE - 1) / TILE_SIZE).max(1) as i32;
        let ty = ((h + TILE_SIZE - 1) / TILE_SIZE).max(1) as i32;

        self.doc_width_px = w;
        self.doc_height_px = h;
        self.doc_tiles_x = tx;
        self.doc_tiles_y = ty;

        self.layers = vec![
            Layer::new_white("Lớp nền (Background)", tx, ty, true),
            Layer::new_transparent("Lớp vẽ 1", tx, ty),
        ];
        self.active_layer = 1;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.stroke_dirty_tiles.clear();
        self.stroke_touched_tiles.clear();
        self.file_path = None;
        self.pan = Vec2::ZERO;
        self.zoom = 1.0;
        self.composite_cache.clear();
        self.mark_full_composite();
        self.set_status(format!("Canvas mới: {w} × {h} px"));
    }

    // ─── Thao tác vẽ (Paint Engine) ──────────────────────────────
    fn paint_stamp(&mut self, cx: f32, cy: f32, color: [f32; 4]) {
        let r = self.brush_radius;
        if r <= 0.0 {
            return;
        }
        let r_sq = r * r;
        let hardness = self.brush_hardness.clamp(0.0, 1.0);
        let opacity = self.brush_opacity;
        let soft_range = (1.0 - hardness).max(0.001);
        let is_eraser = color[3] <= 0.0;

        let min_x = (cx - r).floor() as i32;
        let max_x = (cx + r).ceil() as i32;
        let min_y = (cy - r).floor() as i32;
        let max_y = (cy + r).ceil() as i32;
        let max_doc_x = self.doc_width() as i32;
        let max_doc_y = self.doc_height() as i32;

        if self.active_layer >= self.layers.len() {
            return;
        }
        if self.layers[self.active_layer].locked {
            return;
        }

        let tile_size_i = TILE_SIZE as i32;
        // Duyệt theo tile thay vì theo pixel → 1 lần HashMap lookup / tile
        let tmin_x = min_x.div_euclid(tile_size_i);
        let tmax_x = max_x.div_euclid(tile_size_i);
        let tmin_y = min_y.div_euclid(tile_size_i);
        let tmax_y = max_y.div_euclid(tile_size_i);

        let mut touched: Vec<(i32, i32)> = Vec::new();

        for ty in tmin_y..=tmax_y {
            for tx in tmin_x..=tmax_x {
                let coord = TileCoord::new(tx, ty);
                let layer = &mut self.layers[self.active_layer];
                let Some(tile) = layer.tiles.get_mut(&coord) else {
                    continue;
                };

                // Snapshot undo 1 lần / tile (giữ f32 thô trong stroke; nén zstd khi commit)
                self.stroke_dirty_tiles
                    .entry(coord)
                    .or_insert_with(|| tile.data.clone());
                touched.push((tx, ty));

                let base_x = tx * tile_size_i;
                let base_y = ty * tile_size_i;

                // Clip: brush AABB ∩ tile bounds ∩ document bounds
                let px0 = min_x.max(base_x).max(0);
                let px1 = max_x.min(base_x + tile_size_i - 1).min(max_doc_x - 1);
                let py0 = min_y.max(base_y).max(0);
                let py1 = max_y.min(base_y + tile_size_i - 1).min(max_doc_y - 1);
                if px0 > px1 || py0 > py1 {
                    continue;
                }

                for py in py0..=py1 {
                    let ly = (py - base_y) as usize;
                    let dy = py as f32 - cy;
                    let dy_sq = dy * dy;
                    for px in px0..=px1 {
                        let lx = (px - base_x) as usize;
                        let dx = px as f32 - cx;
                        let dist_sq = dx * dx + dy_sq;
                        if dist_sq > r_sq {
                            continue;
                        }

                        let t = dist_sq.sqrt() / r;
                        let falloff = if t <= hardness {
                            1.0
                        } else {
                            let s = (t - hardness) / soft_range;
                            (1.0 - s * s).max(0.0)
                        };
                        let alpha = opacity * falloff * if is_eraser { 1.0 } else { color[3] };
                        if alpha < 0.001 {
                            continue;
                        }

                        let existing = tile.get_pixel(lx, ly);
                        if is_eraser {
                            let new_a = (existing[3] - alpha).max(0.0);
                            tile.set_pixel(lx, ly, [existing[0], existing[1], existing[2], new_a]);
                        } else {
                            let inv = 1.0 - alpha;
                            tile.set_pixel(
                                lx,
                                ly,
                                [
                                    color[0] * alpha + existing[0] * inv,
                                    color[1] * alpha + existing[1] * inv,
                                    color[2] * alpha + existing[2] * inv,
                                    alpha + existing[3] * inv,
                                ],
                            );
                        }
                    }
                }
            }
        }

        // Dirty Tile Blit: chỉ đánh dấu tile cọ chạm
        for (tx, ty) in touched {
            self.stroke_touched_tiles.insert((tx, ty));
            self.mark_tile_dirty(tx, ty);
        }
    }

    fn paint_stroke(&mut self, from: Pos2, to: Pos2, color: [f32; 4]) {
        let dist = (to - from).length();
        // Bước ~ 0.35× radius: giảm overdraw nặng so với 0.25 mà vẫn mượt
        let step = (self.brush_radius * 0.35).max(1.0);
        let steps = (dist / step).ceil() as usize;
        if steps <= 1 {
            self.paint_stamp(to.x, to.y, color);
            return;
        }
        let inv = 1.0 / steps as f32;
        for i in 0..=steps {
            let t = i as f32 * inv;
            let pos = from + (to - from) * t;
            self.paint_stamp(pos.x, pos.y, color);
        }
    }

    fn undo(&mut self) {
        if let Some(step) = self.undo_stack.pop() {
            let mut redo_tiles = HashMap::with_capacity(step.tiles.len());
            let mut dirty_coords = Vec::with_capacity(step.tiles.len());
            if let Some(layer) = self.layers.get_mut(step.layer_idx) {
                for (coord, compressed_original) in step.tiles {
                    if let Some(tile) = layer.tiles.get_mut(&coord) {
                        // Nén trạng thái hiện tại cho redo, giải nén bản gốc vào tile
                        let current = std::mem::replace(
                            &mut tile.data,
                            compressed_original.decompress(),
                        );
                        redo_tiles.insert(coord, CompressedTile::compress(&current));
                        dirty_coords.push(coord);
                    }
                }
            }
            self.redo_stack.push(UndoStep {
                name: step.name,
                layer_idx: step.layer_idx,
                tiles: redo_tiles,
            });
            // Dirty blit chỉ các tile bị undo — không full composite
            for coord in dirty_coords {
                self.mark_tile_dirty(coord.x, coord.y);
            }
        }
    }

    fn redo(&mut self) {
        if let Some(step) = self.redo_stack.pop() {
            let mut undo_tiles = HashMap::with_capacity(step.tiles.len());
            let mut dirty_coords = Vec::with_capacity(step.tiles.len());
            if let Some(layer) = self.layers.get_mut(step.layer_idx) {
                for (coord, compressed_new) in step.tiles {
                    if let Some(tile) = layer.tiles.get_mut(&coord) {
                        let current =
                            std::mem::replace(&mut tile.data, compressed_new.decompress());
                        undo_tiles.insert(coord, CompressedTile::compress(&current));
                        dirty_coords.push(coord);
                    }
                }
            }
            self.redo_stack.push(UndoStep {
                name: step.name,
                layer_idx: step.layer_idx,
                tiles: undo_tiles,
            });
            // Dirty blit chỉ các tile bị redo
            for coord in dirty_coords {
                self.mark_tile_dirty(coord.x, coord.y);
            }
        }
    }

    fn clear_canvas(&mut self) {
        let layer_idx = self.active_layer;
        let mut old_tiles = HashMap::new();
        if let Some(layer) = self.layers.get_mut(layer_idx) {
            if layer.locked {
                return;
            }
            old_tiles.reserve(layer.tiles.len());
            for (coord, tile) in layer.tiles.iter_mut() {
                let len = tile.data.len();
                let prev = std::mem::replace(
                    &mut tile.data,
                    vec![0.0f32; len].into_boxed_slice(),
                );
                old_tiles.insert(*coord, CompressedTile::compress(&prev));
            }
        }
        self.undo_stack.push(UndoStep {
            name: "Xóa sạch Canvas".to_string(),
            layer_idx,
            tiles: old_tiles,
        });
        self.redo_stack.clear();
        self.mark_full_composite();
    }
}

// ─── Vẽ biểu tượng công cụ (Tool Icons) ──────────────────────────
fn draw_tool_icon(painter: &egui::Painter, tool: Tool, rect: Rect, stroke: Stroke) {
    let c = rect.center();
    let s = 8.0_f32;

    match tool {
        Tool::Move => {
            painter.line_segment([c - Vec2::new(s, 0.0), c + Vec2::new(s, 0.0)], stroke);
            painter.line_segment([c - Vec2::new(0.0, s), c + Vec2::new(0.0, s)], stroke);
            painter.line_segment([c + Vec2::new(s, 0.0), c + Vec2::new(s - 3.0, -3.0)], stroke);
            painter.line_segment([c + Vec2::new(s, 0.0), c + Vec2::new(s - 3.0, 3.0)], stroke);
        }
        Tool::Marquee => {
            let r = Rect::from_center_size(c, Vec2::new(s * 1.6, s * 1.6));
            painter.rect_stroke(r, CornerRadius::ZERO, stroke, egui::StrokeKind::Outside);
        }
        Tool::Lasso => {
            painter.circle_stroke(c - Vec2::new(0.0, 2.0), s * 0.7, stroke);
            painter.line_segment([c + Vec2::new(0.0, 3.0), c + Vec2::new(4.0, 8.0)], stroke);
        }
        Tool::Crop => {
            let r = Rect::from_center_size(c, Vec2::new(s * 1.4, s * 1.4));
            painter.line_segment([r.left_top() - Vec2::new(3.0, 0.0), r.right_top()], stroke);
            painter.line_segment([r.right_top(), r.right_bottom() + Vec2::new(0.0, 3.0)], stroke);
            painter.line_segment([r.left_bottom(), r.right_bottom() + Vec2::new(3.0, 0.0)], stroke);
            painter.line_segment([r.left_top() - Vec2::new(0.0, 3.0), r.left_bottom()], stroke);
        }
        Tool::Eyedropper => {
            let tip = c + Vec2::new(-s * 0.7, s * 0.7);
            let top = c + Vec2::new(s * 0.7, -s * 0.7);
            painter.line_segment([tip, top], stroke);
            painter.circle_filled(top, s * 0.35, stroke.color);
        }
        Tool::Brush => {
            let tip = c + Vec2::new(-s * 0.7, s * 0.7);
            let handle = c + Vec2::new(s * 0.7, -s * 0.7);
            painter.line_segment([tip, handle], stroke);
            painter.line_segment([tip, tip + Vec2::new(-2.0, 2.0)], Stroke::new(stroke.width * 2.0, stroke.color));
        }
        Tool::Eraser => {
            let r = Rect::from_center_size(c, Vec2::new(s * 1.5, s * 1.0));
            painter.rect_stroke(r, CornerRadius::same(2), stroke, egui::StrokeKind::Outside);
            painter.line_segment([r.left_center(), r.right_center()], stroke);
        }
        Tool::Bucket => {
            let p1 = c + Vec2::new(-s * 0.6, -s * 0.4);
            let p2 = c + Vec2::new(s * 0.4, -s * 0.8);
            let p3 = c + Vec2::new(s * 0.8, s * 0.4);
            let p4 = c + Vec2::new(-s * 0.2, s * 0.8);
            painter.line_segment([p1, p2], stroke);
            painter.line_segment([p2, p3], stroke);
            painter.line_segment([p3, p4], stroke);
            painter.line_segment([p4, p1], stroke);
            painter.circle_filled(c + Vec2::new(-s * 0.6, s * 0.8), 2.0, stroke.color);
        }
        Tool::Hand => {
            painter.rect_stroke(Rect::from_center_size(c, Vec2::new(s * 1.2, s * 1.4)), CornerRadius::same(3), stroke, egui::StrokeKind::Outside);
            painter.line_segment([c - Vec2::new(s * 0.3, s * 0.6), c - Vec2::new(s * 0.3, s * 0.1)], stroke);
        }
        Tool::Zoom => {
            painter.circle_stroke(c - Vec2::new(2.0, 2.0), s * 0.6, stroke);
            painter.line_segment([c + Vec2::new(2.0, 2.0), c + Vec2::new(s * 0.9, s * 0.9)], stroke);
        }
        Tool::Text => {
            painter.line_segment([c - Vec2::new(s * 0.7, s * 0.8), c + Vec2::new(s * 0.7, -s * 0.8)], stroke);
            painter.line_segment([c - Vec2::new(0.0, s * 0.8), c + Vec2::new(0.0, s * 0.8)], stroke);
        }
    }
}

// ─── Xem trước nét vẽ cọ (Brush Preview) ─────────────────────────
fn render_brush_preview(painter: &egui::Painter, rect: Rect, color: [f32; 4], radius: f32) {
    painter.rect_filled(rect, CornerRadius::same(4), Color32::from_rgb(18, 18, 20));
    painter.rect_stroke(rect, CornerRadius::same(4), Stroke::new(1.0_f32, Color32::from_rgb(45, 45, 50)), egui::StrokeKind::Outside);

    let p0 = Pos2::new(rect.min.x + 20.0, rect.max.y - 18.0);
    let p1 = Pos2::new(rect.min.x + rect.width() * 0.35, rect.min.y + 12.0);
    let p2 = Pos2::new(rect.min.x + rect.width() * 0.65, rect.max.y - 12.0);
    let p3 = Pos2::new(rect.max.x - 20.0, rect.min.y + 18.0);

    let steps = 60;
    let base_r = radius.clamp(2.0, 24.0);

    let [r, g, b, _] = color;
    let c32 = Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8);

    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let u = 1.0 - t;
        let pos = Pos2::new(
            u * u * u * p0.x + 3.0 * u * u * t * p1.x + 3.0 * u * t * t * p2.x + t * t * t * p3.x,
            u * u * u * p0.y + 3.0 * u * u * t * p1.y + 3.0 * u * t * t * p2.y + t * t * t * p3.y,
        );

        let width_factor = (t * std::f32::consts::PI).sin().powf(0.6);
        let curr_r = base_r * width_factor;

        painter.circle_filled(pos, curr_r, Color32::from_rgba_unmultiplied(c32.r(), c32.g(), c32.b(), 220));
    }
}

// ─── eframe::App Thực thi ────────────────────────────────────────
impl eframe::App for DepPhotoApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ── Phím tắt bàn phím (1 lần đọc input thay vì 12+ closure) ──
        let (
            cmd_z,
            cmd_shift,
            cmd_y,
            cmd_n,
            cmd_o,
            cmd_s,
            cmd_j,
            key_b,
            key_e,
            key_h,
            key_i,
            key_m,
            key_v,
            key_z_tool,
            key_x,
        ) = ctx.input(|i| {
            let cmd = i.modifiers.command;
            (
                cmd && i.key_pressed(egui::Key::Z),
                i.modifiers.shift,
                cmd && i.key_pressed(egui::Key::Y),
                cmd && i.key_pressed(egui::Key::N),
                cmd && i.key_pressed(egui::Key::O),
                cmd && i.key_pressed(egui::Key::S),
                cmd && i.key_pressed(egui::Key::J),
                i.key_pressed(egui::Key::B),
                i.key_pressed(egui::Key::E),
                i.key_pressed(egui::Key::H),
                i.key_pressed(egui::Key::I),
                i.key_pressed(egui::Key::M),
                i.key_pressed(egui::Key::V),
                i.key_pressed(egui::Key::Z) && !cmd,
                i.key_pressed(egui::Key::X),
            )
        });

        if cmd_z {
            if cmd_shift {
                self.redo();
            } else {
                self.undo();
            }
        }
        if cmd_y {
            self.redo();
        }
        if cmd_n {
            self.new_canvas_dialog = Some(NewCanvasDialog {
                width: self.doc_width_px as u32,
                height: self.doc_height_px as u32,
            });
        }
        if cmd_o {
            if let Some(path) = rfd::FileDialog::new().pick_file() {
                self.open_image_file(path);
            }
        }
        if cmd_s {
            if cmd_shift {
                self.save_file_as(Some("png"));
            } else {
                self.save_file();
            }
        }
        if cmd_j {
            self.duplicate_active_layer();
        }
        if key_b {
            self.current_tool = Tool::Brush;
        }
        if key_e {
            self.current_tool = Tool::Eraser;
        }
        if key_h {
            self.current_tool = Tool::Hand;
        }
        if key_i {
            self.current_tool = Tool::Eyedropper;
        }
        if key_m {
            self.current_tool = Tool::Marquee;
        }
        if key_v {
            self.current_tool = Tool::Move;
        }
        if key_z_tool {
            self.current_tool = Tool::Zoom;
        }
        if key_x {
            std::mem::swap(&mut self.fg_color, &mut self.bg_color);
        }

        // ── 1. Thanh Menu trên cùng (Top Menu Bar) ──
        egui::TopBottomPanel::top("top_menu")
            .exact_height(28.0)
            .frame(egui::Frame::side_top_panel(&ctx.style()).fill(Color32::from_rgb(22, 22, 24)))
            .show(ctx, |ui| {
                egui::menu::bar(ui, |ui| {
                    ui.menu_button("Tập tin", |ui| {
                        if ui.button("Canvas mới… (Ctrl+N)").clicked() {
                            self.new_canvas_dialog = Some(NewCanvasDialog {
                                width: self.doc_width_px as u32,
                                height: self.doc_height_px as u32,
                            });
                            ui.close_menu();
                        }
                        if ui.button("Mở ảnh… (Ctrl+O)").clicked() {
                            if let Some(path) = rfd::FileDialog::new().pick_file() {
                                self.open_image_file(path);
                            }
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Lưu ảnh (Ctrl+S)").clicked() {
                            self.save_file();
                            ui.close_menu();
                        }
                        if ui.button("Lưu ảnh dưới dạng… (Ctrl+Shift+S)").clicked() {
                            self.save_file_as(None);
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Xuất ảnh định dạng PNG…").clicked() {
                            self.save_file_as(Some("png"));
                            ui.close_menu();
                        }
                        if ui.button("Xuất ảnh định dạng JPEG…").clicked() {
                            self.save_file_as(Some("jpg"));
                            ui.close_menu();
                        }
                        if ui.button("Xuất ảnh định dạng WebP…").clicked() {
                            self.save_file_as(Some("webp"));
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Đặt lại Canvas mặc định (768×768)").clicked() {
                            *self = DepPhotoApp::new();
                            ui.close_menu();
                        }
                    });

                    ui.menu_button("Chỉnh sửa", |ui| {
                        if ui.add_enabled(!self.undo_stack.is_empty(), egui::Button::new("Hoàn tác (Ctrl+Z)")).clicked() {
                            self.undo();
                            ui.close_menu();
                        }
                        if ui.add_enabled(!self.redo_stack.is_empty(), egui::Button::new("Làm lại (Ctrl+Y)")).clicked() {
                            self.redo();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Xóa sạch Canvas").clicked() {
                            self.clear_canvas();
                            ui.close_menu();
                        }
                    });

                    ui.menu_button("Lớp ảnh", |ui| {
                        if ui.button("Tạo lớp mới (➕)").clicked() {
                            self.add_new_layer();
                            ui.close_menu();
                        }
                        if ui.button("Nhân bản lớp (Ctrl+J)").clicked() {
                            self.duplicate_active_layer();
                            ui.close_menu();
                        }
                        if ui.add_enabled(self.layers.len() > 1, egui::Button::new("Xóa lớp này (🗑)")).clicked() {
                            self.delete_active_layer();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.add_enabled(self.active_layer + 1 < self.layers.len(), egui::Button::new("Chuyển lên trên (▲)")).clicked() {
                            self.move_active_layer_up();
                            ui.close_menu();
                        }
                        if ui.add_enabled(self.active_layer > 0, egui::Button::new("Chuyển xuống dưới (▼)")).clicked() {
                            self.move_active_layer_down();
                            ui.close_menu();
                        }
                    });

                    ui.menu_button("Vùng nhìn", |ui| {
                        if ui.button("Phóng to (➕)").clicked() {
                            self.zoom = (self.zoom * 1.25).clamp(0.1, 10.0);
                            ui.close_menu();
                        }
                        if ui.button("Thu nhỏ (➖)").clicked() {
                            self.zoom = (self.zoom / 1.25).clamp(0.1, 10.0);
                            ui.close_menu();
                        }
                        if ui.button("Tỷ lệ chuẩn 100%").clicked() {
                            self.zoom = 1.0;
                            ui.close_menu();
                        }
                        if ui.button("Vừa màn hình").clicked() {
                            self.zoom = 1.0;
                            self.pan = Vec2::ZERO;
                            ui.close_menu();
                        }
                        ui.separator();
                        ui.checkbox(&mut self.show_grid, "Hiển thị lưới ô Tile");
                    });

                    ui.menu_button("Cửa sổ", |ui| {
                        ui.checkbox(&mut self.show_color_palette_window, "Bảng chọn màu");
                        ui.checkbox(&mut self.show_tips_window, "Mẹo & Phím tắt cho người mới");
                    });

                    ui.menu_button("Trợ giúp", |ui| {
                        if ui.button("💡 Mẹo & Phím tắt cho người mới").clicked() {
                            self.show_tips_window = true;
                            ui.close_menu();
                        }
                        ui.separator();
                        ui.label(egui::RichText::new("DEP_PHOTO v1.2.0 (Rust + egui)").weak());
                    });

                    // Thống kê góc phải trên cùng
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(format!("Thu phóng: {:.0}%", self.zoom * 100.0)).size(11.0).weak());
                        ui.separator();
                        ui.label(egui::RichText::new(format!("Kích thước: {}×{} px ({} ô tile)", self.doc_width(), self.doc_height(), self.doc_tiles_x * self.doc_tiles_y)).size(11.0).weak());
                        ui.separator();
                        ui.label(egui::RichText::new("DEP_PHOTO v1.2.0").size(11.0).color(Color32::from_rgb(140, 140, 150)));
                    });
                });
            });

        // ── 2. Thanh thuộc tính công cụ (Tool Options Bar) ──
        egui::TopBottomPanel::top("tool_options_bar")
            .exact_height(32.0)
            .frame(egui::Frame::side_top_panel(&ctx.style()).fill(Color32::from_rgb(26, 26, 29)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(4.0);
                    let tool_label = format!("🛠 {}", self.current_tool.short_name());
                    ui.label(egui::RichText::new(tool_label).strong().size(12.0).color(Color32::from_rgb(0, 180, 255)));
                    ui.separator();

                    match self.current_tool {
                        Tool::Brush | Tool::Eraser => {
                            ui.label(egui::RichText::new("Cỡ cọ:").size(11.5).weak());
                            ui.add(egui::Slider::new(&mut self.brush_radius, 1.0..=100.0).suffix(" px"));

                            ui.label(egui::RichText::new("Độ cứng:").size(11.5).weak());
                            ui.add(egui::Slider::new(&mut self.brush_hardness, 0.0..=1.0).custom_formatter(|n, _| format!("{:.0}%", n * 100.0)));

                            ui.label(egui::RichText::new("Độ mờ:").size(11.5).weak());
                            ui.add(egui::Slider::new(&mut self.brush_opacity, 0.01..=1.0).custom_formatter(|n, _| format!("{:.0}%", n * 100.0)));

                            ui.separator();
                            ui.label(egui::RichText::new("Cỡ nhanh:").size(11.0).weak());
                            for &sz in &[3.0, 8.0, 16.0, 32.0, 64.0] {
                                if ui.selectable_label((self.brush_radius - sz).abs() < 0.1, format!("{sz:.0}")).clicked() {
                                    self.brush_radius = sz;
                                }
                            }
                        }
                        Tool::Eyedropper => {
                            ui.label(egui::RichText::new("Màu lấy mẫu:").size(11.5).weak());
                            let fg_c32 = Color32::from_rgb(
                                (self.fg_color[0] * 255.0) as u8,
                                (self.fg_color[1] * 255.0) as u8,
                                (self.fg_color[2] * 255.0) as u8,
                            );
                            let (swatch_rect, _) = ui.allocate_exact_size(Vec2::new(18.0, 18.0), egui::Sense::hover());
                            ui.painter().rect_filled(swatch_rect, CornerRadius::same(2), fg_c32);
                            ui.painter().rect_stroke(swatch_rect, CornerRadius::same(2), Stroke::new(1.0_f32, Color32::WHITE), egui::StrokeKind::Outside);
                            ui.label(egui::RichText::new(format!("#{:02X}{:02X}{:02X}", fg_c32.r(), fg_c32.g(), fg_c32.b())).monospace().size(11.0));
                            ui.label(egui::RichText::new("(Click chuột trái vào ảnh để trích xuất màu)").size(11.0).weak());
                        }
                        Tool::Hand => {
                            ui.label(egui::RichText::new("Kéo chuột trái trên Canvas để di chuyển vùng nhìn").size(11.5).weak());
                            if ui.button("Về giữa").clicked() {
                                self.pan = Vec2::ZERO;
                            }
                        }
                        Tool::Zoom => {
                            if ui.button("➕ Phóng to").clicked() {
                                self.zoom = (self.zoom * 1.25).clamp(0.1, 10.0);
                            }
                            if ui.button("➖ Thu nhỏ").clicked() {
                                self.zoom = (self.zoom / 1.25).clamp(0.1, 10.0);
                            }
                            if ui.button("100%").clicked() {
                                self.zoom = 1.0;
                            }
                            if ui.button("Vừa khung hình").clicked() {
                                self.zoom = 1.0;
                                self.pan = Vec2::ZERO;
                            }
                        }
                        _ => {
                            ui.label(egui::RichText::new("Mẹo: Nhấn B để vẽ, E để tẩy, H để cuộn màn hình, Z để thu phóng").size(11.5).weak());
                        }
                    }

                    // Điều khiển bên phải thanh công cụ
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Nút bật cửa sổ Mẹo & Phím tắt nhanh
                        if ui.button(egui::RichText::new("💡 Mẹo hay").color(Color32::from_rgb(255, 215, 0))).on_hover_text("Bấm xem các phím tắt và mẹo vẽ hữu ích cho người mới").clicked() {
                            self.show_tips_window = !self.show_tips_window;
                        }
                        ui.separator();
                        let grid_text = if self.show_grid { "▦ Lưới ô (Bật)" } else { "▦ Lưới ô (Tắt)" };
                        if ui.selectable_label(self.show_grid, grid_text).on_hover_text("Bật hoặc tắt đường chia lưới ô Tile").clicked() {
                            self.show_grid = !self.show_grid;
                        }
                        ui.separator();
                        if ui.button(format!("{:.0}%", self.zoom * 100.0)).on_hover_text("Bấm để đưa tỷ lệ về 100%").clicked() {
                            self.zoom = 1.0;
                        }
                    });
                });
            });

        // ── 3. Thanh trạng thái chân trang (Bottom Status Bar) ──
        egui::TopBottomPanel::bottom("status_bar")
            .exact_height(24.0)
            .frame(egui::Frame::side_top_panel(&ctx.style()).fill(Color32::from_rgb(20, 20, 22)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let pos_str = if let Some(p) = self.mouse_doc_pos {
                        format!("Tọa độ: {:.0}, {:.0} px", p.x, p.y)
                    } else {
                        "Tọa độ: 0, 0 px".to_string()
                    };
                    ui.label(egui::RichText::new(pos_str).size(11.0).weak());
                    ui.separator();
                    ui.label(egui::RichText::new(format!("Kích thước: {} × {} px", self.doc_width(), self.doc_height())).size(11.0).weak());
                    ui.separator();
                    ui.label(egui::RichText::new(format!("Hệ màu: RGB Linear | Bộ nhớ: {:.1} MB", self.total_memory_mb())).size(11.0).weak());

                    if let Some((msg, time)) = &self.status_message {
                        if time.elapsed().as_secs() < 4 {
                            ui.separator();
                            ui.label(egui::RichText::new(msg).size(11.0).color(Color32::from_rgb(100, 220, 140)));
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let file_title = self.file_path
                            .as_ref()
                            .and_then(|p| p.file_name())
                            .and_then(|n| n.to_str())
                            .unwrap_or("Tác phẩm chưa đặt tên (Untitled)");
                        ui.label(egui::RichText::new(file_title).size(11.0).weak());
                    });
                });
            });

        // ── 4. Thanh công cụ bên trái (Left Toolbar) ──
        egui::SidePanel::left("left_toolbar")
            .exact_width(40.0)
            .resizable(false)
            .frame(egui::Frame::side_top_panel(&ctx.style()).fill(Color32::from_rgb(26, 26, 28)))
            .show(ctx, |ui| {
                ui.add_space(4.0);
                for tool in ALL_TOOLS {
                    let is_active = self.current_tool == tool;
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(32.0, 30.0), egui::Sense::click());

                    let bg = if is_active {
                        Color32::from_rgb(0, 162, 237)
                    } else if resp.hovered() {
                        Color32::from_rgb(42, 42, 48)
                    } else {
                        Color32::TRANSPARENT
                    };

                    ui.painter().rect_filled(rect, CornerRadius::same(3), bg);

                    let stroke_color = if is_active {
                        Color32::WHITE
                    } else if resp.hovered() {
                        Color32::from_rgb(240, 240, 250)
                    } else {
                        Color32::from_rgb(160, 160, 170)
                    };

                    draw_tool_icon(ui.painter(), tool, rect, Stroke::new(1.4_f32, stroke_color));

                    if resp.clicked() {
                        self.current_tool = tool;
                    }
                    if resp.hovered() {
                        resp.on_hover_text(tool.label());
                    }
                    ui.add_space(2.0);
                }

                // Hộp chọn màu kép (Tiền cảnh & Hậu cảnh)
                ui.add_space(14.0);
                let (swatch_rect, swatch_resp) = ui.allocate_exact_size(Vec2::new(32.0, 32.0), egui::Sense::click());

                let fg_c32 = Color32::from_rgb(
                    (self.fg_color[0] * 255.0) as u8,
                    (self.fg_color[1] * 255.0) as u8,
                    (self.fg_color[2] * 255.0) as u8,
                );
                let bg_c32 = Color32::from_rgb(
                    (self.bg_color[0] * 255.0) as u8,
                    (self.bg_color[1] * 255.0) as u8,
                    (self.bg_color[2] * 255.0) as u8,
                );

                let bg_box = Rect::from_min_size(swatch_rect.min + Vec2::new(10.0, 10.0), Vec2::new(18.0, 18.0));
                ui.painter().rect_filled(bg_box, CornerRadius::ZERO, bg_c32);
                ui.painter().rect_stroke(bg_box, CornerRadius::ZERO, Stroke::new(1.0_f32, Color32::from_rgb(80, 80, 90)), egui::StrokeKind::Outside);

                let fg_box = Rect::from_min_size(swatch_rect.min + Vec2::new(2.0, 2.0), Vec2::new(18.0, 18.0));
                ui.painter().rect_filled(fg_box, CornerRadius::ZERO, fg_c32);
                ui.painter().rect_stroke(fg_box, CornerRadius::ZERO, Stroke::new(1.0_f32, Color32::from_rgb(90, 90, 100)), egui::StrokeKind::Outside);

                if swatch_resp.clicked() {
                    std::mem::swap(&mut self.fg_color, &mut self.bg_color);
                }
                if swatch_resp.hovered() {
                    swatch_resp.on_hover_text("Bấm để đổi màu Vẽ / màu Nền (Phím tắt: X)");
                }
            });

        // ── 5. Bảng điều khiển bên phải (Right Panel) ──
        egui::SidePanel::right("right_panel")
            .exact_width(260.0)
            .resizable(false)
            .frame(egui::Frame::side_top_panel(&ctx.style()).fill(Color32::from_rgb(28, 28, 30)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    // ── A. Thuộc tính cọ vẽ ──
                    ui.label(egui::RichText::new("Thuộc tính Bút vẽ").size(12.5).strong());
                    ui.add_space(4.0);

                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Bán kính").size(11.0).weak());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new(format!("{:.1} px", self.brush_radius)).size(11.0));
                        });
                    });
                    ui.add(egui::Slider::new(&mut self.brush_radius, 1.0..=100.0).show_value(false));

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Độ mờ đục").size(11.0).weak());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new(format!("{:.0}%", self.brush_opacity * 100.0)).size(11.0));
                        });
                    });
                    ui.add(egui::Slider::new(&mut self.brush_opacity, 0.01..=1.0).show_value(false));

                    // ── B. Xem trước nét cọ ──
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Xem trước nét cọ").size(12.0).strong());
                    ui.add_space(2.0);

                    let (preview_rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 65.0), egui::Sense::hover());
                    render_brush_preview(ui.painter(), preview_rect, self.fg_color, self.brush_radius);

                    ui.vertical_centered(|ui| {
                        let (dot_rect, _) = ui.allocate_exact_size(Vec2::new(12.0, 12.0), egui::Sense::hover());
                        ui.painter().circle_filled(dot_rect.center(), (self.brush_radius * 0.25).clamp(1.5, 5.0), Color32::WHITE);
                    });

                    // ── C. Quản lý lớp ảnh (Layers) ──
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Quản lý Lớp (Layers)").size(12.5).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new(format!("{} lớp", self.layers.len())).weak().size(11.0));
                        });
                    });
                    ui.add_space(4.0);

                    // Độ mờ của layer đang chọn
                    if let Some(active_layer) = self.layers.get_mut(self.active_layer) {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Độ mờ lớp này").size(11.0).weak());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(egui::RichText::new(format!("{:.0}%", active_layer.opacity * 100.0)).size(11.0));
                            });
                        });
                        if ui.add(egui::Slider::new(&mut active_layer.opacity, 0.0..=1.0).show_value(false)).changed() {
                            self.mark_full_composite();
                        }
                    }

                    ui.add_space(4.0);
                    // Thanh thao tác nhanh với layer
                    ui.horizontal(|ui| {
                        if ui.button("➕").on_hover_text("Thêm Lớp mới").clicked() {
                            self.add_new_layer();
                        }
                        if ui.button("📋").on_hover_text("Nhân bản Lớp (Ctrl+J)").clicked() {
                            self.duplicate_active_layer();
                        }
                        let can_delete = self.layers.len() > 1;
                        if ui.add_enabled(can_delete, egui::Button::new("🗑")).on_hover_text("Xóa Lớp này").clicked() {
                            self.delete_active_layer();
                        }
                        ui.separator();
                        let can_up = self.active_layer + 1 < self.layers.len();
                        if ui.add_enabled(can_up, egui::Button::new("▲")).on_hover_text("Chuyển Lớp lên trên").clicked() {
                            self.move_active_layer_up();
                        }
                        let can_down = self.active_layer > 0;
                        if ui.add_enabled(can_down, egui::Button::new("▼")).on_hover_text("Chuyển Lớp xuống dưới").clicked() {
                            self.move_active_layer_down();
                        }
                        if let Some(l) = self.layers.get_mut(self.active_layer) {
                            let lock_icon = if l.locked { "🔒" } else { "🔓" };
                            if ui.button(lock_icon).on_hover_text(if l.locked { "Mở khóa Lớp" } else { "Khóa Lớp này lại" }).clicked() {
                                l.locked = !l.locked;
                            }
                        }
                    });
                    ui.add_space(4.0);

                    // Danh sách các layer
                    for i in (0..self.layers.len()).rev() {
                        let is_active = i == self.active_layer;
                        let visible = self.layers[i].visible;
                        let name = self.layers[i].name.clone();
                        let locked = self.layers[i].locked;

                        let bg = if is_active {
                            Color32::from_rgb(42, 42, 46)
                        } else {
                            Color32::from_rgb(32, 32, 34)
                        };

                        egui::Frame::NONE
                            .fill(bg)
                            .corner_radius(CornerRadius::same(3))
                            .inner_margin(egui::Margin::symmetric(6, 6))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let eye_text = if visible { "👁" } else { " " };
                                    if ui.button(egui::RichText::new(eye_text).size(12.0)).on_hover_text("Ẩn / Hiện lớp này").clicked() {
                                        self.layers[i].visible = !visible;
                                        self.mark_full_composite();
                                    }

                                    let (thumb_rect, _) = ui.allocate_exact_size(Vec2::new(18.0, 18.0), egui::Sense::hover());
                                    ui.painter().rect_filled(thumb_rect, CornerRadius::ZERO, Color32::WHITE);
                                    ui.painter().rect_stroke(thumb_rect, CornerRadius::ZERO, Stroke::new(1.0_f32, Color32::from_rgb(100, 100, 100)), egui::StrokeKind::Outside);

                                    let label = egui::RichText::new(format!("• {}", name)).size(11.5).color(if is_active {
                                        Color32::WHITE
                                    } else {
                                        Color32::from_rgb(190, 190, 200)
                                    });

                                    if ui.selectable_label(is_active, label).clicked() {
                                        self.active_layer = i;
                                    }

                                    if locked {
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.label(egui::RichText::new("🔒").size(10.0).weak());
                                        });
                                    }
                                });
                            });
                        ui.add_space(2.0);
                    }

                    // ── D. Lịch sử thao tác (History) ──
                    ui.add_space(12.0);
                    ui.label(egui::RichText::new("Lịch sử thao tác").size(12.5).strong());
                    ui.add_space(2.0);

                    egui::Frame::NONE
                        .fill(Color32::from_rgb(34, 34, 38))
                        .corner_radius(CornerRadius::same(3))
                        .inner_margin(egui::Margin::symmetric(8, 6))
                        .show(ui, |ui| {
                            let last_action = self.undo_stack.last().map(|s| s.name.as_str()).unwrap_or("Canvas mới");
                            ui.label(egui::RichText::new(last_action).size(11.5).strong());
                        });

                    ui.add_space(10.0);
                    ui.vertical_centered(|ui| {
                        let (star_rect, _) = ui.allocate_exact_size(Vec2::new(32.0, 32.0), egui::Sense::hover());
                        let c = star_rect.center();
                        let s = 12.0_f32;
                        let stroke = Stroke::new(1.5_f32, Color32::from_rgb(60, 60, 68));
                        painter_star(ui.painter(), c, s, stroke);
                    });
                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        let btn_w = (ui.available_width() - 6.0) * 0.5;
                        if ui.add_sized([btn_w, 24.0], egui::Button::new("Hoàn tác")).on_hover_text("Ctrl+Z").clicked() {
                            self.undo();
                        }
                        if ui.add_sized([btn_w, 24.0], egui::Button::new("Làm lại")).on_hover_text("Ctrl+Y").clicked() {
                            self.redo();
                        }
                    });

                    ui.add_space(4.0);
                    if ui.add_sized([ui.available_width(), 24.0], egui::Button::new("Xóa sạch Canvas")).clicked() {
                        self.clear_canvas();
                    }
                });
            });

        // ── 6. Vùng vẽ trung tâm (Central Canvas Viewport) ──
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::from_rgb(22, 22, 24)))
            .show(ctx, |ui| {
                if self.texture_dirty || self.doc_texture.is_none() {
                    self.update_texture(ctx);
                }

                let (response, painter) =
                    ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
                let canvas_rect = response.rect;

                // Thu phóng & Lia chuột (Zoom & Pan)
                if response.hovered() {
                    let zoom_delta = ui.input(|i| i.zoom_delta());
                    if zoom_delta != 1.0 {
                        self.zoom = (self.zoom * zoom_delta).clamp(0.1, 10.0);
                    }
                    let scroll = ui.input(|i| i.smooth_scroll_delta);
                    if scroll.y != 0.0 && ui.input(|i| i.modifiers.ctrl) {
                        let factor = (scroll.y * 0.003).exp();
                        self.zoom = (self.zoom * factor).clamp(0.1, 10.0);
                    }
                }

                if response.dragged_by(egui::PointerButton::Middle)
                    || response.dragged_by(egui::PointerButton::Secondary)
                    || (self.current_tool == Tool::Hand && response.dragged_by(egui::PointerButton::Primary))
                {
                    self.pan += response.drag_delta();
                }

                let doc_w = self.doc_width() as f32 * self.zoom;
                let doc_h = self.doc_height() as f32 * self.zoom;
                let doc_origin = canvas_rect.center() + self.pan - Vec2::new(doc_w, doc_h) * 0.5;
                let doc_rect = Rect::from_min_size(doc_origin, Vec2::new(doc_w, doc_h));

                // Bóng đổ Canvas
                painter.rect_filled(doc_rect.expand(4.0), CornerRadius::ZERO, Color32::from_rgba_unmultiplied(0, 0, 0, 80));

                // Nền trắng cơ sở
                painter.rect_filled(doc_rect, CornerRadius::ZERO, Color32::WHITE);

                // Hiển thị Texture tổng hợp
                if let Some(tex) = &self.doc_texture {
                    painter.image(
                        tex.id(),
                        doc_rect,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }

                // Vẽ lưới ô Tile (nếu đang bật)
                if self.show_grid {
                    let tile_screen = TILE_SIZE as f32 * self.zoom;
                    let grid_color = Color32::from_rgba_unmultiplied(180, 220, 255, 140);
                    let grid_stroke = Stroke::new(1.0_f32, grid_color);

                    for tx in 1..self.doc_tiles_x {
                        let x = doc_rect.min.x + tx as f32 * tile_screen;
                        if x < doc_rect.max.x {
                            painter.line_segment([Pos2::new(x, doc_rect.min.y), Pos2::new(x, doc_rect.max.y)], grid_stroke);
                        }
                    }
                    for ty in 1..self.doc_tiles_y {
                        let y = doc_rect.min.y + ty as f32 * tile_screen;
                        if y < doc_rect.max.y {
                            painter.line_segment([Pos2::new(doc_rect.min.x, y), Pos2::new(doc_rect.max.x, y)], grid_stroke);
                        }
                    }
                }

                // Viền tài liệu
                painter.rect_stroke(doc_rect, CornerRadius::ZERO, Stroke::new(1.0_f32, Color32::from_rgb(60, 60, 65)), egui::StrokeKind::Outside);

                // Theo dõi tọa độ chuột trên tài liệu
                if response.hovered() {
                    if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
                        let dpx = (pos.x - doc_rect.min.x) / self.zoom;
                        let dpy = (pos.y - doc_rect.min.y) / self.zoom;
                        self.mouse_doc_pos = Some(Pos2::new(dpx, dpy));
                    }
                } else {
                    self.mouse_doc_pos = None;
                }

                // Thao tác vẽ & tô màu
                let is_paint = self.current_tool == Tool::Brush || self.current_tool == Tool::Eraser;
                if is_paint && response.dragged_by(egui::PointerButton::Primary) {
                    if let Some(mouse_screen) = response.interact_pointer_pos() {
                        let doc_px = (mouse_screen.x - doc_rect.min.x) / self.zoom;
                        let doc_py = (mouse_screen.y - doc_rect.min.y) / self.zoom;
                        let pos = Pos2::new(doc_px, doc_py);

                        let color = if self.current_tool == Tool::Eraser {
                            [0.0, 0.0, 0.0, 0.0]
                        } else {
                            self.fg_color
                        };

                        if let Some(prev) = self.last_stroke_pos {
                            self.paint_stroke(prev, pos, color);
                        } else {
                            self.paint_stamp(pos.x, pos.y, color);
                        }
                        self.last_stroke_pos = Some(pos);
                    }
                } else if self.current_tool == Tool::Eyedropper && response.clicked_by(egui::PointerButton::Primary) {
                    if let Some(mouse_screen) = response.interact_pointer_pos() {
                        let dpx = ((mouse_screen.x - doc_rect.min.x) / self.zoom) as i32;
                        let dpy = ((mouse_screen.y - doc_rect.min.y) / self.zoom) as i32;
                        let coord = TileCoord::from_pixel(dpx, dpy);
                        let lx = dpx.rem_euclid(TILE_SIZE as i32) as usize;
                        let ly = dpy.rem_euclid(TILE_SIZE as i32) as usize;
                        if let Some(layer) = self.layers.get(self.active_layer) {
                            if let Some(tile) = layer.tiles.get(&coord) {
                                self.fg_color = tile.get_pixel(lx, ly);
                            }
                        }
                    }
                }

                if response.drag_stopped() {
                    self.last_stroke_pos = None;
                    self.stroke_touched_tiles.clear();
                    if !self.stroke_dirty_tiles.is_empty() {
                        // Nén zstd các tile snapshot khi commit nét → tiết kiệm RAM lịch sử
                        let raw_tiles = std::mem::take(&mut self.stroke_dirty_tiles);
                        let mut compressed = HashMap::with_capacity(raw_tiles.len());
                        for (coord, data) in raw_tiles {
                            compressed.insert(coord, CompressedTile::compress(&data));
                        }
                        self.undo_stack.push(UndoStep {
                            name: "Nét vẽ cọ".to_string(),
                            layer_idx: self.active_layer,
                            tiles: compressed,
                        });
                        self.redo_stack.clear();
                    }
                }
            });

        // ── 7. Cửa sổ nổi: Bảng chọn màu ──
        if self.show_color_palette_window {
            egui::Window::new("Bảng chọn màu")
                .default_pos(Pos2::new(880.0, 360.0))
                .default_size(Vec2::new(160.0, 160.0))
                .resizable(false)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        let (sv_rect, sv_resp) = ui.allocate_exact_size(Vec2::new(115.0, 115.0), egui::Sense::click_and_drag());
                        ui.add_space(4.0);
                        let (hue_rect, hue_resp) = ui.allocate_exact_size(Vec2::new(16.0, 115.0), egui::Sense::click_and_drag());

                        let p = ui.painter();

                        let steps = 14;
                        let w_step = sv_rect.width() / steps as f32;
                        let h_step = sv_rect.height() / steps as f32;

                        for iy in 0..steps {
                            let val = 1.0 - (iy as f32 / steps as f32);
                            for ix in 0..steps {
                                let sat = ix as f32 / steps as f32;
                                let rgb = hsv_to_rgb(self.hsv_hue, sat, val);
                                let tile_box = Rect::from_min_size(
                                    sv_rect.min + Vec2::new(ix as f32 * w_step, iy as f32 * h_step),
                                    Vec2::new(w_step + 1.0, h_step + 1.0),
                                );
                                p.rect_filled(tile_box, CornerRadius::ZERO, rgb);
                            }
                        }

                        let cur_pos = Pos2::new(
                            sv_rect.min.x + self.hsv_sat * sv_rect.width(),
                            sv_rect.min.y + (1.0 - self.hsv_val) * sv_rect.height(),
                        );
                        p.circle_stroke(cur_pos, 4.0, Stroke::new(1.5_f32, Color32::WHITE));
                        p.circle_stroke(cur_pos, 5.0, Stroke::new(1.0_f32, Color32::BLACK));

                        let h_steps = 20;
                        let h_bar_step = hue_rect.height() / h_steps as f32;

                        for iy in 0..h_steps {
                            let hue = iy as f32 / h_steps as f32;
                            let rgb = hsv_to_rgb(hue, 1.0, 1.0);
                            let bar_box = Rect::from_min_size(
                                hue_rect.min + Vec2::new(0.0, iy as f32 * h_bar_step),
                                Vec2::new(hue_rect.width(), h_bar_step + 1.0),
                            );
                            p.rect_filled(bar_box, CornerRadius::ZERO, rgb);
                        }

                        let notch_y = hue_rect.min.y + self.hsv_hue * hue_rect.height();
                        p.rect_filled(
                            Rect::from_center_size(Pos2::new(hue_rect.center().x, notch_y), Vec2::new(hue_rect.width() + 4.0, 3.0)),
                            CornerRadius::ZERO,
                            Color32::WHITE,
                        );

                        if sv_resp.dragged() || sv_resp.clicked() {
                            if let Some(pos) = sv_resp.interact_pointer_pos() {
                                self.hsv_sat = ((pos.x - sv_rect.min.x) / sv_rect.width()).clamp(0.0, 1.0);
                                self.hsv_val = (1.0 - (pos.y - sv_rect.min.y) / sv_rect.height()).clamp(0.0, 1.0);
                                let rgb = hsv_to_rgb(self.hsv_hue, self.hsv_sat, self.hsv_val);
                                self.fg_color = [rgb.r() as f32 / 255.0, rgb.g() as f32 / 255.0, rgb.b() as f32 / 255.0, 1.0];
                            }
                        }

                        if hue_resp.dragged() || hue_resp.clicked() {
                            if let Some(pos) = hue_resp.interact_pointer_pos() {
                                self.hsv_hue = ((pos.y - hue_rect.min.y) / hue_rect.height()).clamp(0.0, 1.0);
                                let rgb = hsv_to_rgb(self.hsv_hue, self.hsv_sat, self.hsv_val);
                                self.fg_color = [rgb.r() as f32 / 255.0, rgb.g() as f32 / 255.0, rgb.b() as f32 / 255.0, 1.0];
                            }
                        }
                    });
                });
        }

        // ── 8. Cửa sổ nổi: Mẹo & Hướng dẫn cho người mới (Tips & Tricks) ──
        if self.show_tips_window {
            egui::Window::new("💡 Mẹo & Phím tắt cho người mới")
                .default_pos(Pos2::new(420.0, 180.0))
                .default_size(Vec2::new(480.0, 380.0))
                .resizable(true)
                .collapsible(true)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.heading("Chào mừng bạn đến với DEP_PHOTO!");
                        ui.label(egui::RichText::new("Dưới đây là các phím tắt và mẹo thao tác giúp bạn làm việc nhanh hơn gấp 3 lần:").size(12.0).weak());
                        ui.add_space(8.0);

                        ui.collapsing("⚡ Phím tắt bàn phím thông dụng nhất", |ui| {
                            egui::Grid::new("shortcuts_grid")
                                .striped(true)
                                .spacing([16.0, 6.0])
                                .show(ui, |ui| {
                                    ui.label(egui::RichText::new("B").strong());
                                    ui.label("Chuyển nhanh sang Bút vẽ (Brush)");
                                    ui.end_row();

                                    ui.label(egui::RichText::new("E").strong());
                                    ui.label("Chuyển nhanh sang Cục tẩy (Eraser)");
                                    ui.end_row();

                                    ui.label(egui::RichText::new("I").strong());
                                    ui.label("Lấy mẫu màu bất kỳ trên ảnh (Eyedropper)");
                                    ui.end_row();

                                    ui.label(egui::RichText::new("X").strong());
                                    ui.label("Hoán đổi giữa màu Vẽ và màu Nền");
                                    ui.end_row();

                                    ui.label(egui::RichText::new("Ctrl + Z").strong());
                                    ui.label("Hoàn tác thao tác vừa vẽ (Undo)");
                                    ui.end_row();

                                    ui.label(egui::RichText::new("Ctrl + Y").strong());
                                    ui.label("Làm lại thao tác vừa hoàn tác (Redo)");
                                    ui.end_row();

                                    ui.label(egui::RichText::new("Ctrl + Cuộn chuột").strong());
                                    ui.label("Phóng to / thu nhỏ Canvas mượt mà");
                                    ui.end_row();

                                    ui.label(egui::RichText::new("Giữ chuột giữa / Phím H").strong());
                                    ui.label("Lia Canvas sang vùng khác (Pan)");
                                    ui.end_row();

                                    ui.label(egui::RichText::new("Ctrl + J").strong());
                                    ui.label("Nhân bản lớp (Duplicate Layer)");
                                    ui.end_row();

                                    ui.label(egui::RichText::new("Ctrl + S").strong());
                                    ui.label("Lưu tác phẩm nhanh");
                                    ui.end_row();
                                });
                        });

                        ui.add_space(8.0);
                        ui.collapsing("🎨 Mẹo phối màu & Quản lý lớp ảnh (Layers)", |ui| {
                            ui.label("• Luôn tạo Layer mới (nút ➕) khi vẽ chi tiết mới để không vẽ đè làm hỏng hình nền gốc.");
                            ui.label("• Dùng nút 📋 (Ctrl+J) để nhân bản layer trước khi thử nghiệm ý tưởng táo bạo.");
                            ui.label("• Giảm 'Độ cứng' (Hardness) của cọ vẽ xuống 20% - 50% để tạo viền chuyển sắc mềm mại, đổ bóng tự nhiên.");
                            ui.label("• Chỉnh thanh trượt 'Độ mờ lớp' (Layer Opacity) để hòa trộn nhẹ nhàng giữa các lớp ảnh.");
                        });

                        ui.add_space(8.0);
                        ui.collapsing("🚀 Tối ưu hiệu năng đồ họa", |ui| {
                            ui.label("• DEP_PHOTO chia ảnh thành các khối Tile 256×256 để xử lý siêu tốc.");
                            ui.label("• Bấm nút '▦ Lưới ô' trên thanh công cụ để ẩn/hiển thị lưới phân mảnh khi cần quan sát tổng thể.");
                        });

                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            if ui.button("  Đã hiểu  ").clicked() {
                                self.show_tips_window = false;
                            }
                        });
                    });
                });
        }

        // ── 9. Hộp thoại Tạo Canvas Mới ──
        if let Some(dialog) = &mut self.new_canvas_dialog {
            let mut close = false;
            let mut create = false;

            egui::Window::new("Tạo Canvas Mới")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label("Chiều rộng (px):");
                        ui.add(egui::DragValue::new(&mut dialog.width).range(64..=8192));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Chiều cao (px):");
                        ui.add(egui::DragValue::new(&mut dialog.height).range(64..=8192));
                    });

                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("Kích thước thông dụng:").size(11.5).weak());
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("768×768 (Vuông chuẩn)").clicked() {
                            dialog.width = 768;
                            dialog.height = 768;
                        }
                        if ui.button("1080×1080 (Vuông HD)").clicked() {
                            dialog.width = 1080;
                            dialog.height = 1080;
                        }
                        if ui.button("1920×1080 (Full HD)").clicked() {
                            dialog.width = 1920;
                            dialog.height = 1080;
                        }
                        if ui.button("1280×720 (HD 720p)").clicked() {
                            dialog.width = 1280;
                            dialog.height = 720;
                        }
                    });

                    let tx = ((dialog.width as usize + TILE_SIZE - 1) / TILE_SIZE).max(1);
                    let ty = ((dialog.height as usize + TILE_SIZE - 1) / TILE_SIZE).max(1);
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "Cấu trúc: {}×{} ô tile 256px (~{:.1} MB bộ nhớ)",
                            tx,
                            ty,
                            (tx * ty * 2) as f32
                        ))
                        .weak()
                        .size(11.0),
                    );

                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("  Tạo Canvas  ").clicked() {
                            create = true;
                        }
                        if ui.button("Hủy bỏ").clicked() {
                            close = true;
                        }
                    });
                });

            if create {
                let (w, h) = (dialog.width, dialog.height);
                self.create_new_canvas(w, h);
                self.new_canvas_dialog = None;
            } else if close {
                self.new_canvas_dialog = None;
            }
        }
    }
}

// ─── Hàm bổ trợ màu sắc & hình học ──────────────────────────────
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> Color32 {
    let i = (h * 6.0).floor() as i32;
    let f = h * 6.0 - i as f32;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);

    let (r, g, b) = match i % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };

    Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

fn painter_star(painter: &egui::Painter, c: Pos2, s: f32, stroke: Stroke) {
    let top = c - Vec2::new(0.0, s);
    let bottom = c + Vec2::new(0.0, s);
    let left = c - Vec2::new(s, 0.0);
    let right = c + Vec2::new(s, 0.0);
    painter.line_segment([top, bottom], stroke);
    painter.line_segment([left, right], stroke);
    let d = s * 0.45;
    painter.line_segment([c - Vec2::new(d, d), c + Vec2::new(d, d)], stroke);
    painter.line_segment([c - Vec2::new(d, -d), c + Vec2::new(d, -d)], stroke);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layer_operations() {
        let mut app = DepPhotoApp::new();
        assert_eq!(app.layers.len(), 2);
        assert_eq!(app.active_layer, 1);

        app.add_new_layer();
        assert_eq!(app.layers.len(), 3);
        assert_eq!(app.active_layer, 2);

        app.duplicate_active_layer();
        assert_eq!(app.layers.len(), 4);

        app.move_active_layer_down();
        assert_eq!(app.active_layer, 2);

        app.move_active_layer_up();
        assert_eq!(app.active_layer, 3);

        app.delete_active_layer();
        assert_eq!(app.layers.len(), 3);
    }

    #[test]
    fn test_composite_pixel_dimensions() {
        let mut app = DepPhotoApp::new();
        app.create_new_canvas(400, 300);
        assert_eq!(app.doc_width(), 400);
        assert_eq!(app.doc_height(), 300);

        let pixels = app.composite_pixels();
        assert_eq!(pixels.len(), 400 * 300);

        let img = app.composite_image();
        assert_eq!(img.width(), 400);
        assert_eq!(img.height(), 300);
    }
}
