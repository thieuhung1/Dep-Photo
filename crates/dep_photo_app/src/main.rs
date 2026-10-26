use std::collections::HashMap;
use dep_photo_core::{Tile, TileCoord, TILE_SIZE};
use eframe::egui::{self, Color32, ColorImage, Pos2, Rect, Stroke, TextureOptions, Vec2};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 768.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("DEP_PHOTO — High Performance Photo Editor (Preview)"),
        ..Default::default()
    };
    eframe::run_native(
        "DEP_PHOTO",
        options,
        Box::new(|cc| {
            // Thiết lập dark mode chuẩn
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(DepPhotoApp::new()))
        }),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Brush,
    Eraser,
    Pan,
    Eyedropper,
}

struct UndoStep {
    tiles: HashMap<TileCoord, Box<[f32]>>,
}

struct DepPhotoApp {
    // Kích thước document tính theo Tile
    doc_tiles_x: i32,
    doc_tiles_y: i32,
    tiles: HashMap<TileCoord, Tile>,

    // Texture cache hiển thị trên canvas
    doc_texture: Option<egui::TextureHandle>,
    texture_dirty: bool,

    // Viewport camera
    zoom: f32,
    pan: Vec2,

    // Tools & Options
    current_tool: Tool,
    brush_color: [f32; 4], // Linear sRGB RGBA
    brush_radius: f32,
    show_tile_grid: bool,

    // Stroke tracking
    last_stroke_pos: Option<Pos2>,
    stroke_dirty_tiles: HashMap<TileCoord, Box<[f32]>>,

    // Undo / Redo
    undo_stack: Vec<UndoStep>,
    redo_stack: Vec<UndoStep>,
}

impl DepPhotoApp {
    fn new() -> Self {
        let doc_tiles_x = 3; // 768px
        let doc_tiles_y = 3; // 768px
        let mut tiles = HashMap::new();

        for ty in 0..doc_tiles_y {
            for tx in 0..doc_tiles_x {
                let mut tile = Tile::new();
                // Khởi tạo nền trắng mờ để phân biệt với vùng ngoài canvas
                for y in 0..TILE_SIZE {
                    for x in 0..TILE_SIZE {
                        tile.set_pixel(x, y, [1.0, 1.0, 1.0, 1.0]);
                    }
                }
                tiles.insert(TileCoord::new(tx, ty), tile);
            }
        }

        Self {
            doc_tiles_x,
            doc_tiles_y,
            tiles,
            doc_texture: None,
            texture_dirty: true,
            zoom: 1.0,
            pan: Vec2::ZERO,
            current_tool: Tool::Brush,
            brush_color: [0.15, 0.5, 0.95, 1.0], // Xanh dương
            brush_radius: 8.0,
            show_tile_grid: true,
            last_stroke_pos: None,
            stroke_dirty_tiles: HashMap::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    fn doc_width(&self) -> usize {
        (self.doc_tiles_x as usize) * TILE_SIZE
    }

    fn doc_height(&self) -> usize {
        (self.doc_tiles_y as usize) * TILE_SIZE
    }

    /// Cập nhật texture từ các Tile vào egui texture
    fn update_texture(&mut self, ctx: &egui::Context) {
        let width = self.doc_width();
        let height = self.doc_height();
        let mut pixels = vec![Color32::TRANSPARENT; width * height];

        for ty in 0..self.doc_tiles_y {
            for tx in 0..self.doc_tiles_x {
                if let Some(tile) = self.tiles.get(&TileCoord::new(tx, ty)) {
                    let origin_x = (tx as usize) * TILE_SIZE;
                    let origin_y = (ty as usize) * TILE_SIZE;

                    for y in 0..TILE_SIZE {
                        let dst_row = (origin_y + y) * width;
                        for x in 0..TILE_SIZE {
                            let [r, g, b, a] = tile.get_pixel(x, y);
                            // Chuyển đổi Linear sRGB -> 8-bit sRGB hiển thị
                            let cr = (r.clamp(0.0, 1.0) * 255.0) as u8;
                            let cg = (g.clamp(0.0, 1.0) * 255.0) as u8;
                            let cb = (b.clamp(0.0, 1.0) * 255.0) as u8;
                            let ca = (a.clamp(0.0, 1.0) * 255.0) as u8;
                            pixels[dst_row + origin_x + x] = Color32::from_rgba_unmultiplied(cr, cg, cb, ca);
                        }
                    }
                }
            }
        }

        let image = ColorImage {
            size: [width, height],
            pixels,
        };

        match &mut self.doc_texture {
            Some(tex) => tex.set(image, TextureOptions::NEAREST),
            None => {
                self.doc_texture = Some(ctx.load_texture("dep_canvas", image, TextureOptions::NEAREST));
            }
        }
        self.texture_dirty = false;
    }

    /// Vẽ một điểm cọ (Brush stamp) vào các Tile bị ảnh hưởng
    fn paint_stamp(&mut self, center_x: f32, center_y: f32, color: [f32; 4]) {
        let r = self.brush_radius;
        let r_sq = r * r;
        let min_x = (center_x - r).floor() as i32;
        let max_x = (center_x + r).ceil() as i32;
        let min_y = (center_y - r).floor() as i32;
        let max_y = (center_y + r).ceil() as i32;

        let max_doc_x = self.doc_width() as i32;
        let max_doc_y = self.doc_height() as i32;

        for py in min_y..=max_y {
            if py < 0 || py >= max_doc_y {
                continue;
            }
            let dy = py as f32 - center_y;
            let dy_sq = dy * dy;

            for px in min_x..=max_x {
                if px < 0 || px >= max_doc_x {
                    continue;
                }
                let dx = px as f32 - center_x;
                let dist_sq = dx * dx + dy_sq;
                if dist_sq > r_sq {
                    continue;
                }

                // Tính tọa độ Tile
                let coord = TileCoord::from_pixel(px, py);
                let local_x = px.rem_euclid(TILE_SIZE as i32) as usize;
                let local_y = py.rem_euclid(TILE_SIZE as i32) as usize;

                if let Some(tile) = self.tiles.get_mut(&coord) {
                    // Lưu lại bản sao tile trước khi sửa cho Undo nếu chưa lưu
                    self.stroke_dirty_tiles
                        .entry(coord)
                        .or_insert_with(|| tile.data.clone());

                    tile.set_pixel(local_x, local_y, color);
                }
            }
        }
        self.texture_dirty = true;
    }

    /// Nội suy giữa 2 điểm chuột để nét vẽ liền mạch không bị đứt
    fn paint_stroke(&mut self, from: Pos2, to: Pos2, color: [f32; 4]) {
        let dist = (to - from).length();
        let step = (self.brush_radius * 0.4).max(1.0);
        let steps = (dist / step).ceil() as usize;

        if steps <= 1 {
            self.paint_stamp(to.x, to.y, color);
        } else {
            for i in 0..=steps {
                let t = i as f32 / steps as f32;
                let pos = from + (to - from) * t;
                self.paint_stamp(pos.x, pos.y, color);
            }
        }
    }

    fn undo(&mut self) {
        if let Some(step) = self.undo_stack.pop() {
            let mut redo_tiles = HashMap::new();
            for (coord, original_data) in step.tiles {
                if let Some(tile) = self.tiles.get_mut(&coord) {
                    redo_tiles.insert(coord, tile.data.clone());
                    tile.data = original_data;
                }
            }
            self.redo_stack.push(UndoStep { tiles: redo_tiles });
            self.texture_dirty = true;
        }
    }

    fn redo(&mut self) {
        if let Some(step) = self.redo_stack.pop() {
            let mut undo_tiles = HashMap::new();
            for (coord, new_data) in step.tiles {
                if let Some(tile) = self.tiles.get_mut(&coord) {
                    undo_tiles.insert(coord, tile.data.clone());
                    tile.data = new_data;
                }
            }
            self.undo_stack.push(UndoStep { tiles: undo_tiles });
            self.texture_dirty = true;
        }
    }
}

impl eframe::App for DepPhotoApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Phím tắt bàn phím
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z)) {
            if ctx.input(|i| i.modifiers.shift) {
                self.redo();
            } else {
                self.undo();
            }
        } else if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Y)) {
            self.redo();
        } else if ctx.input(|i| i.key_pressed(egui::Key::B)) {
            self.current_tool = Tool::Brush;
        } else if ctx.input(|i| i.key_pressed(egui::Key::E)) {
            self.current_tool = Tool::Eraser;
        } else if ctx.input(|i| i.key_pressed(egui::Key::H)) {
            self.current_tool = Tool::Pan;
        }

        // 1. Menu bar trên cùng
        egui::TopBottomPanel::top("top_menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New Document (768x768)").clicked() {
                        *self = DepPhotoApp::new();
                        ui.close_menu();
                    }
                });
                ui.menu_button("Edit", |ui| {
                    if ui.add_enabled(!self.undo_stack.is_empty(), egui::Button::new("Undo (Ctrl+Z)")).clicked() {
                        self.undo();
                        ui.close_menu();
                    }
                    if ui.add_enabled(!self.redo_stack.is_empty(), egui::Button::new("Redo (Ctrl+Y)")).clicked() {
                        self.redo();
                        ui.close_menu();
                    }
                });
                ui.menu_button("View", |ui| {
                    if ui.button("Reset View (100%)").clicked() {
                        self.zoom = 1.0;
                        self.pan = Vec2::ZERO;
                        ui.close_menu();
                    }
                    ui.checkbox(&mut self.show_tile_grid, "Show Tile Grid (256x256)");
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("Zoom: {:.0}%", self.zoom * 100.0));
                    ui.separator();
                    ui.label(format!("Doc: {}x{} px ({} tiles)", self.doc_width(), self.doc_height(), self.tiles.len()));
                    ui.separator();
                    ui.label("DEP_PHOTO v0.1.0");
                });
            });
        });

        // 2. Toolbar bên trái
        egui::SidePanel::left("left_tools").exact_width(48.0).resizable(false).show(ctx, |ui| {
            ui.add_space(4.0);
            if ui.selectable_label(self.current_tool == Tool::Brush, "🖌\nBrush").clicked() {
                self.current_tool = Tool::Brush;
            }
            if ui.selectable_label(self.current_tool == Tool::Eraser, "🧹\nEraser").clicked() {
                self.current_tool = Tool::Eraser;
            }
            if ui.selectable_label(self.current_tool == Tool::Pan, "✋\nPan").clicked() {
                self.current_tool = Tool::Pan;
            }
            if ui.selectable_label(self.current_tool == Tool::Eyedropper, "💉\nPick").clicked() {
                self.current_tool = Tool::Eyedropper;
            }
        });

        // 3. Panel điều khiển bên phải
        egui::SidePanel::right("right_panel").default_width(220.0).show(ctx, |ui| {
            ui.heading("Tool Settings");
            ui.separator();

            ui.add(egui::Slider::new(&mut self.brush_radius, 1.0..=50.0).text("Radius (px)"));

            ui.add_space(8.0);
            ui.heading("Color");
            let mut srgba = [
                (self.brush_color[0] * 255.0) as u8,
                (self.brush_color[1] * 255.0) as u8,
                (self.brush_color[2] * 255.0) as u8,
            ];
            if ui.color_edit_button_srgb(&mut srgba).changed() {
                self.brush_color[0] = srgba[0] as f32 / 255.0;
                self.brush_color[1] = srgba[1] as f32 / 255.0;
                self.brush_color[2] = srgba[2] as f32 / 255.0;
            }

            ui.add_space(16.0);
            ui.heading("Layers");
            ui.separator();
            ui.label("• Layer 1 (Raster)");

            ui.add_space(16.0);
            ui.heading("History");
            ui.label(format!("Undo steps: {}", self.undo_stack.len()));
            ui.label(format!("Redo steps: {}", self.redo_stack.len()));
            if ui.button("Clear Canvas").clicked() {
                for tile in self.tiles.values_mut() {
                    for y in 0..TILE_SIZE {
                        for x in 0..TILE_SIZE {
                            tile.set_pixel(x, y, [1.0, 1.0, 1.0, 1.0]);
                        }
                    }
                }
                self.texture_dirty = true;
            }
        });

        // 4. Viewport / Canvas trung tâm
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.texture_dirty || self.doc_texture.is_none() {
                self.update_texture(ctx);
            }

            let (response, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
            let canvas_rect = response.rect;

            // Xử lý Pan và Zoom
            if response.hovered() {
                let zoom_delta = ui.input(|i| i.zoom_delta());
                if zoom_delta != 1.0 {
                    self.zoom = (self.zoom * zoom_delta).clamp(0.1, 10.0);
                }
                let scroll = ui.input(|i| i.smooth_scroll_delta);
                if scroll != Vec2::ZERO && ui.input(|i| i.modifiers.ctrl) {
                    let factor = (scroll.y * 0.002).exp();
                    self.zoom = (self.zoom * factor).clamp(0.1, 10.0);
                }
            }

            if response.dragged_by(egui::PointerButton::Middle)
                || response.dragged_by(egui::PointerButton::Secondary)
                || (self.current_tool == Tool::Pan && response.dragged_by(egui::PointerButton::Primary))
            {
                self.pan += response.drag_delta();
            }

            // Tọa độ hiển thị document trên canvas
            let doc_w = self.doc_width() as f32 * self.zoom;
            let doc_h = self.doc_height() as f32 * self.zoom;
            let center_screen = canvas_rect.center() + self.pan;
            let doc_rect = Rect::from_center_size(center_screen, Vec2::new(doc_w, doc_h));

            // Vẽ nền checkerboard trong suốt và viền document
            painter.rect_filled(canvas_rect, 0.0, Color32::from_rgb(30, 30, 32));
            painter.rect_filled(doc_rect, 0.0, Color32::WHITE);

            // Vẽ Texture Document
            if let Some(tex) = &self.doc_texture {
                painter.image(
                    tex.id(),
                    doc_rect,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }

            // Vẽ lưới Tile Grid (256x256)
            if self.show_tile_grid {
                let tile_screen_size = TILE_SIZE as f32 * self.zoom;
                for tx in 0..=self.doc_tiles_x {
                    let x = doc_rect.min.x + tx as f32 * tile_screen_size;
                    painter.line_segment(
                        [Pos2::new(x, doc_rect.min.y), Pos2::new(x, doc_rect.max.y)],
                        Stroke::new(1.0, Color32::from_rgba_unmultiplied(80, 140, 255, 120)),
                    );
                }
                for ty in 0..=self.doc_tiles_y {
                    let y = doc_rect.min.y + ty as f32 * tile_screen_size;
                    painter.line_segment(
                        [Pos2::new(doc_rect.min.x, y), Pos2::new(doc_rect.max.x, y)],
                        Stroke::new(1.0, Color32::from_rgba_unmultiplied(80, 140, 255, 120)),
                    );
                }
            }

            painter.rect_stroke(doc_rect, 0.0, Stroke::new(1.5, Color32::from_rgb(100, 100, 100)));

            // Xử lý vẽ nét cọ (Painting)
            if (self.current_tool == Tool::Brush || self.current_tool == Tool::Eraser)
                && response.dragged_by(egui::PointerButton::Primary)
            {
                if let Some(mouse_screen) = response.interact_pointer_pos() {
                    // Chuyển đổi tọa độ màn hình sang tọa độ Document Pixel
                    let doc_px = (mouse_screen.x - doc_rect.min.x) / self.zoom;
                    let doc_py = (mouse_screen.y - doc_rect.min.y) / self.zoom;
                    let current_doc_pos = Pos2::new(doc_px, doc_py);

                    let color = if self.current_tool == Tool::Eraser {
                        [1.0, 1.0, 1.0, 1.0] // Xóa về nền trắng
                    } else {
                        self.brush_color
                    };

                    if let Some(prev) = self.last_stroke_pos {
                        self.paint_stroke(prev, current_doc_pos, color);
                    } else {
                        self.paint_stamp(current_doc_pos.x, current_doc_pos.y, color);
                    }
                    self.last_stroke_pos = Some(current_doc_pos);
                }
            } else if response.drag_stopped() {
                self.last_stroke_pos = None;
                if !self.stroke_dirty_tiles.is_empty() {
                    // Đẩy bước Undo
                    let tiles = std::mem::take(&mut self.stroke_dirty_tiles);
                    self.undo_stack.push(UndoStep { tiles });
                    self.redo_stack.clear();
                }
            }
        });
    }
}
