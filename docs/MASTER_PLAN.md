# DEP_PHOTO — MASTER PROJECT PLAN & TECHNICAL BLUEPRINT
**Trình Chỉnh Sửa Ảnh Hiệu Năng Cao Viết Bằng Rust**

---

## 1. TỔNG QUAN DỰ ÁN & MỤC TIÊU CHIẾN LƯỢC

### 1.1. Tuyên ngôn sứ mệnh
**DEP_PHOTO** là một phần mềm xử lý đồ họa và ảnh kỹ thuật số đa nền tảng (Windows, Linux, macOS) được xây dựng hoàn toàn bằng **Rust**, tận dụng kiến trúc **GPU-first (wgpu/WGSL)** và hệ thống phân mảnh **Tile-based (256x256)** nhằm đem lại hiệu năng vượt trội, độ tin cậy tuyệt đối (Memory-safety) và tốc độ phản hồi 60 FPS thực tế ngay cả trên các file đồ họa cực lớn (50MP – 100MP).

### 1.2. Phân tầng sản phẩm (Product Tiering)
* **DEP_PHOTO Free (MPL 2.0 - Open Source):**
  * Hướng tới người dùng cá nhân, sinh viên, cộng đồng sáng tạo mở.
  * Hỗ trợ đầy đủ: Layer, Group, Layer Mask, Selection, Brush, Basic Filters/Adjustments, định dạng phổ biến (PNG, JPG, WebP) và định dạng gốc `.dpx`.
* **DEP_PHOTO Pro (Commercial License):**
  * Hướng tới Designer, Retoucher, Studio, Nhiếp ảnh gia chuyên nghiệp.
  * Tính năng nâng cao: Smart Object, PSD I/O, RAW Decoder, 16-bit & 32-bit Float, Color Management (ICC Profiles, CMYK), Plugin Engine (WASM), AI Retouch Tools, Batch Processing.
  * Mô hình giá: $79 Lifetime hoặc $5/tháng (Cá nhân); $149/seat (Doanh nghiệp/Studio).

### 1.3. Chỉ số KPI Hiệu năng Bất biến (Hard Performance Budgets)
| Hạng mục | KPI Mục tiêu | Phương pháp đo lường |
| :--- | :--- | :--- |
| **Mở ảnh 50MP** | `< 2.0s` | Criterion benchmark cold-read + decode + tile-init |
| **Mở ảnh 100MP** | `< 5.0s` | Memory-mapped I/O + Multithreaded Tile Generation |
| **Zoom / Pan Canvas** | `60 FPS (16.6ms)` | Viewport Render Loop (wgpu surface present) |
| **Độ trễ nét vẽ (Brush latency)** | `< 16ms` | Input polling -> GPU dirty tile blit |
| **Undo / Redo** | `< 50ms` | Tile Diff decompression & swap |
| **Composite 20 Layer 4K** | `< 8ms / frame` | GPU Render Graph Compute Pass |
| **RAM sử dụng (100MP, 5 Layers)** | `< 2.5 GB` | LRU Tile Cache + Disk Paging |
| **Autosave** | Hoàn toàn nền (0ms lag UI) | Background worker thread with snapshot COW tiles |

---

## 2. THIẾT KẾ KIẾN TRÚC HỆ THỐNG (SYSTEM ARCHITECTURE)

### 2.1. Cấu trúc Cargo Workspace
```text
dep_photo/
├── Cargo.toml                  # Workspace root configuration
├── crates/
│   ├── dep_photo_core/         # Document model, Layer tree, Tile Coordinate, Color, Math
│   ├── dep_photo_engine/       # wgpu context, GPU Tile Cache, Render Graph, Shaders (WGSL)
│   ├── dep_photo_io/           # File decoders/encoders (PNG, JPG, WebP, DPX container, PSD)
│   ├── dep_photo_tools/        # Brush engine, Selection tools, Transform, Color picker
│   ├── dep_photo_filters/      # Image processing algorithms, Compute filters (Blur, Curves)
│   ├── dep_photo_app/          # egui/eframe GUI, Viewport canvas, Keybindings, Menus
│   ├── dep_photo_plugins/      # WASM / C-ABI plugin host runtime
│   ├── dep_photo_ai/           # ONNX / TensorRT / DirectML inference engine
│   └── dep_photo_license/      # License key validation, HWID hashing (Pro tier)
├── shaders/                    # WGSL Compute & Fragment shaders
├── tests/                      # End-to-end integration tests & Fuzzing
└── benches/                    # Criterion performance benchmarks
```

### 2.2. Luồng dữ liệu (Dataflow Pipeline)
```text
[User Input: Mouse/Stylus/Keyboard]
             │
             ▼
[Active Tool (e.g., Brush / Transform / Selection)]
             │ Generates Command & Modified Tiles
             ▼
[Command System (Undo/Redo History)]
             │ Emits TileDiffCommand / StructuralCommand
             ▼
[Document Model (Layer Arena & Sparse Tile Grid)]
             │ Marks updated tile indices
             ▼
[Dirty Tile Tracker]
             │ Traverses modified tiles down the DAG
             ▼
[Render Graph Scheduler]
             │ Dispatches parallel compute passes
             ▼
[GPU Composite Pipeline (wgpu / WGSL)]
             │ Blend modes, Masks, Adjustment Shaders
             ▼
[Viewport Canvas Surface (egui/wgpu)]
```

### 2.3. Các quyết định kỹ thuật then chốt (Architectural Decision Records - ADR)
* **ADR-001: Working Color Space:**
  * Toàn bộ pipeline tổng hợp màu nội bộ dùng **Linear sRGB Float32** (`[0.0, 1.0]`).
  * Tránh hiện tượng clipping màu và sai lệch gamma khi hòa trộn (Blend Modes) như sRGB 8-bit.
* **ADR-002: Sparse Tile System:**
  * Mỗi layer được phân cắt thành lưới tile **256 × 256 pixel**.
  * Các vùng không có dữ liệu (pixel trong suốt) không tốn bộ nhớ RAM (Sparse Allocation bằng `FxHashMap<TileCoord, Arc<Tile>>`).
  * Hỗ trợ Mipmap / Pyramid Level of Detail (LOD 0, LOD 1, LOD 2...) để zoom-out mượt mà mà không cần nạp toàn bộ ảnh gốc.
* **ADR-003: Undo/Redo bằng Tile Diff:**
  * Không bao giờ sao chép toàn bộ Document.
  * Chỉ nén và lưu trữ các tile bị thay đổi (`TileDiff`) bằng thuật toán **Zstandard (zstd)**.
  * Giới hạn bộ nhớ lịch sử: 100 bước hoặc 512 MB. Khi vượt ngưỡng, các bước cũ được swap tạm ra disk scratch.
* **ADR-004: GPU-First Compute Shader Pipeline:**
  * Thay vì dùng Fragment Shader vẽ từng quad, sử dụng **WGSL Compute Shaders** với workgroup `16 × 16` (256 threads tương ứng 1 tile 256x256).
  * Cho phép truy cập bộ nhớ linh hoạt, thực hiện Gaussian Blur 2 chiều, Blend modes và Color Grading trong cùng một dispatch pass.
* **ADR-005: Định dạng tập tin gốc `.dpx`:**
  * Sử dụng cấu trúc container nén (Zip/Tar + Zstd) gồm:
    * `manifest.json`: Metadata document, layer hierarchy, blend modes, opacity, masks.
    * `layers/<layer_id>/tile_<x>_<y>.bin`: Dữ liệu tile dạng nén raw/float.
    * `thumbs/preview.webp`: Ảnh thu nhỏ phục vụ OS preview và mở nhanh.
    * `color/profile.icc`: ICC profile nhúng kèm.
    * `checksums.json`: SHA-256 xác thực tính toàn vẹn dữ liệu.

---

## 3. LỘ TRÌNH THỰC HIỆN CHI TIẾT THEO TỪNG GIAI ĐOẠN

```mermaid
gantt
    title Lộ trình Triển khai DEP_PHOTO (18 - 24 Tháng)
    dateFormat  YYYY-MM
    section Nền Tảng (Core)
    Phase 0: Foundation & Core Engine   :p0, 2026-11, 1M
    section Phiên Bản Free
    Phase 1: MVP Free (Layer, Tool, DPX):p1, after p0, 4M
    Phase 2: Free Stabilization & Polish:p2, after p1, 3M
    section Phiên Bản Pro
    Phase 3: Pro Foundation (16/32bit, PSD, SmartObject):p3, after p2, 5M
    Phase 4: Pro Advanced (RAW, CMYK, Plugin, Batch)   :p4, after p3, 5M
    section AI & Phát Hành
    Phase 5: AI Module (Remove BG, Upscale, Inpaint)  :p5, after p4, 3M
    Phase 6: Release & Commercial Launch              :p6, after p5, 2M
```

---

### GIAI ĐOẠN 0: NỀN TẢNG & ENGINE CỐT LÕI (Tháng 1)
**Mục tiêu:** Xây dựng khung kiến trúc, hệ thống Tile, GPU Context wgpu, Canvas hiển thị 60 FPS với Pan/Zoom và cọ vẽ cơ bản.

#### Sprint 0.1: Workspace Setup & Infrastructure
* Thiết lập Cargo Workspace với các crates chuẩn hóa.
* Cấu hình CI/CD trên GitHub Actions: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`, `cargo deny check`.
* Tích hợp `tracing` và `tracing-subscriber` cho logging hiệu năng cao.
* Thiết lập hệ thống benchmark cơ sở với `criterion`.

#### Sprint 0.2: Sparse Tile System & Memory Allocator
* Định nghĩa cấu trúc `Tile<T>` (256x256), `TileCoord { x: i32, y: i32, lod: u8 }`.
* Hiện thực `TilePool` (Object Pool) để tái sử dụng buffer bộ nhớ, loại bỏ phân mảnh heap khi vẽ liên tục.
* Thiết lập `TileStore` dựa trên `FxHashMap` và `Arc<RwLock<TileBuffer>>`.

#### Sprint 0.3: wgpu GPU Pipeline & Compute Shaders
* Khởi tạo `wgpu::Device`, `wgpu::Queue`, `wgpu::Surface`.
* Viết compute shader đầu tiên bằng WGSL: Composite 2 tile với Normal Blend Mode.
* Thiết kế `GpuTileCache`: Texture 2D Array hoặc Texture Atlas quản lý các tile đang hiển thị trong Viewport.

#### Sprint 0.4: Viewport Canvas & Basic Brush
* Tích hợp canvas render trong `egui` sử dụng custom painting callback của `eframe`.
* Hiện thực ma trận biến đổi Viewport (Pan, Zoom từ 1% đến 3200% với bilinear/nearest filter).
* Hiện thực nét vẽ cơ bản (Circular Brush Stamp nội suy theo khoảng cách chuột) vẽ trực tiếp lên Tile.
* Tích hợp Undo/Redo nguyên mẫu lưu TileDiff.

**Tiêu chí nghiệm thu (DoD Phase 0):**
* Zoom/Pan một canvas 8K mượt mà đạt chuẩn 60 FPS.
* Nét vẽ không bị răng cưa (interpolation đầy đủ), độ trễ dưới 16ms.
* Undo/Redo nét vẽ hoạt động ổn định trong vòng 50ms.

---

### GIAI ĐOẠN 1: BẢN MVP FREE (Tháng 2 – 4)
**Mục tiêu:** Hoàn thiện mô hình Document nhiều layer, hệ thống mask, công cụ vùng chọn, lưu/mở file `.dpx`, filter cơ bản.

#### Sprint 1.1: Document Model & Layer Tree
* Thiết kế Layer Tree bằng Arena Allocation (`slotmap` hoặc `generational-arena`), tránh con trỏ vòng.
* Hỗ trợ các kiểu Layer: `RasterLayer`, `GroupLayer`, `LayerMask`.
* Hiện thực Layer Opacity, Blend Modes chuẩn (Normal, Multiply, Screen, Overlay, Add, Darken, Lighten, Soft Light, Hard Light).

#### Sprint 1.2: Dirty Tile Tracker & Render Graph
* Hiện thực thuật toán đánh dấu Tile bị bẩn (`mark_dirty(tile_coord)`).
* Xây dựng Render Graph: Khi 1 tile ở layer 2 thay đổi, chỉ biên dịch và composite lại các tile tương ứng ở các layer nằm phía trên trong bounding box nhìn thấy.

#### Sprint 1.3: Bộ công cụ Vẽ & Chọn (Free Tools)
* **Drawing:** Brush (hỗ trợ Spacing, Hardness, Opacity), Pencil, Eraser.
* **Selection:** Selection Mask (1-bit hoặc 8-bit alpha), Rectangle Select, Ellipse Select, Free Lasso.
* **Transform cơ bản:** Move Tool, Canvas Crop, Layer Rotate/Scale (Nearest & Bilinear).
* **Color:** Eyedropper (hút màu Linear sRGB), Bucket Fill (Flood Fill trên Tile).

#### Sprint 1.4: Định dạng tập tin `.dpx` V1 & IO Codecs
* Hiện thực parser/encoder định dạng `.dpx` sử dụng `zip` + `zstd`.
* Hỗ trợ Import/Export định dạng ảnh phẳng: PNG, JPEG, WebP thông qua thư viện `image` và `zune-image`.
* Lưu trữ metadata, thumbnail preview trong `.dpx`.

#### Sprint 1.5: Compute Filters Cơ bản
* Viết WGSL Compute Shaders cho các bộ lọc:
  * Brightness / Contrast.
  * Hue / Saturation / Value.
  * Gaussian Blur 2-pass (Horizontal + Vertical Separable Convolution).
  * Unsharp Mask / Sharpen.

**Tiêu chí nghiệm thu (DoD Phase 1):**
* Tạo file ảnh 10 layer, vẽ, tạo mask, áp dụng Gaussian Blur xem trước thời gian thực.
* Lưu thành công file `.dpx`, đóng app, mở lại nguyên vẹn cấu trúc layer và dữ liệu.
* Đạt độ ổn định 0 crash qua 1000 lượt thao tác ngẫu nhiên (Fuzz test).

---

### GIAI ĐOẠN 2: ỔN ĐỊNH HÓA BẢN FREE & HOÀN THIỆN TRẢI NGHIỆM (Tháng 5 – 6)
**Mục tiêu:** Đưa bản Free đạt trạng thái "Production-Ready", giao diện cao cấp, tự động lưu an toàn, bộ chỉnh màu chuyên sâu.

#### Sprint 2.1: Bộ lọc Màu Chuyên Sâu (Adjustments)
* Hiện thực Adjustment Layer:
  * Curves (Spline nội suy Bezier, GPU 1D LUT texture).
  * Levels (Black point, Midtones gamma, White point, Output levels).
  * Color Balance.
* Histogram Engine: GPU reduction compute shader tính toán histogram 256 bin của viewport trong `< 2ms`.

#### Sprint 2.2: Clipping Mask & Tinh Chỉnh Blend Modes
* Hiện thực Clipping Mask (Layer con chỉ hiển thị trong diện tích pixel của Layer mẹ).
* Tối ưu hóa toàn bộ các công thức Blend Mode bằng SIMD trên CPU và vector compute trên GPU.

#### Sprint 2.3: Non-blocking Autosave & Crash Recovery
* Thiết kế kiến trúc Snapshot Copy-on-Write (COW):
  * Khi kích hoạt Autosave, thread chính chỉ freeze con trỏ Tile tree (`Arc` clone cực nhanh `< 1ms`).
  * Worker thread nền thực hiện nén và ghi ra thư mục recovery tạm.
* Cơ chế tự động khôi phục dự án sau khi mất điện hoặc crash ứng dụng đột ngột.

#### Sprint 2.4: Stress Testing & Đạt Chuẩn Hiệu Năng
* Benchmark trên các tệp mẫu 50MP (8688 x 5792):
  * Thời gian mở ảnh `< 2.0s`.
  * RAM tiêu thụ `< 1.5GB`.
* Hoàn thiện UI/UX: Dark Mode tinh tế, Floating dock panels, Phím tắt tương thích Photoshop (B, E, V, M, L, Ctrl+Z, Ctrl+T...).

**Tiêu chí nghiệm thu (DoD Phase 2):**
* Phát hành bản **DEP_PHOTO Free v1.0 (Community Release)**.
* Đầy đủ bộ cài đặt tự động cho Windows (.msi), Linux (.AppImage/.deb), macOS (.dmg).

---

### GIAI ĐOẠN 3: NỀN TẢNG BẢN PRO (Tháng 7 – 10)
**Mục tiêu:** Mở rộng kiến trúc sang độ sâu màu 16-bit / 32-bit Float, hỗ trợ đọc file PSD của Adobe, Smart Object, và ICC Profile.

#### Sprint 3.1: 16-Bit Int & 32-Bit Float Color Pipeline
* Tổng quát hóa `TileBuffer`:
  * `TileBuffer::U8(Vec<u8>)` (8-bit sRGB).
  * `TileBuffer::U16(Vec<u16>)` (16-bit Pro).
  * `TileBuffer::F32(Vec<f32>)` (32-bit HDR / Float).
* Cấu hình lại các Texture format trong `wgpu`: `Rgba16Unorm`, `Rgba32Float`.

#### Sprint 3.2: Color Management & ICC Profiles
* Tích hợp `lcms2` (Little CMS) thông qua Rust binding an toàn.
* Tự động chuyển đổi profile màu từ không gian nguồn (Adobe RGB, DCI-P3, ProPhoto RGB) sang Working Space Linear sRGB.
* Soft-Proofing: Hiển thị mô phỏng màu in ấn trực tiếp trên GPU.

#### Sprint 3.3: Bộ đọc định dạng Adobe PSD (`dep_photo_io`)
* Xây dựng engine dịch ngược và parse cấu trúc file PSD:
  * Header, Color Mode Data, Image Resources.
  * Layer and Mask Information section.
  * Giải nén dữ liệu nén RLE (PackBits) và Zip.
* Nhập chính xác phân cấp Layer, Mask, Opacity và Blend Mode từ Photoshop sang DEP_PHOTO.

#### Sprint 3.4: Smart Object Engine
* Thiết kế `SmartObjectLayer`: Lưu trữ một document con độc lập (`SubDocument`).
* Áp dụng Affine Transformation (Scale, Rotate, Skew) không phá hủy (Non-destructive): Pixel gốc luôn được bảo toàn, chỉ tính toán ma trận ánh xạ khi render.

**Tiêu chí nghiệm thu (DoD Phase 3):**
* Mở được các file `.psd` đồ họa chuyên nghiệp kích thước 2GB với hàng chục layer.
* Làm việc mượt mà ở chế độ màu 16-bit / 32-bit mà không suy giảm tốc độ Viewport 60 FPS.

---

### GIAI ĐOẠN 4: TÍNH NĂNG PRO NÂNG CAO & HỆ SINH THÁI (Tháng 11 – 14)
**Mục tiêu:** Bộ công cụ Retouching cao cấp, giải mã ảnh RAW máy ảnh, xuất PSD, CMYK và hệ thống Plugin WASM.

#### Sprint 4.1: Retouching Engine Chuyên Nghiệp
* **Clone Stamp Tool:** Lấy mẫu từ điểm nguồn (Source coordinate) sang điểm đích thời gian thực với xem trước con trỏ (Stamp preview overlay).
* **Healing Brush Tool:** Thuật toán giải phương trình Poisson (Poisson Image Editing) trên GPU để hòa trộn kết cấu da và ánh sáng liền mạch.
* **Liquify / Perspective Warp:** Biến dạng lưới (Mesh Grid Warping) thực thi bằng GPU Compute.

#### Sprint 4.2: Camera RAW Decoder
* Tích hợp thư viện giải mã RAW chuyên dụng (`libraw` hoặc Rust native decoder).
* Hỗ trợ giải nén RAW từ Canon (.CR2, .CR3), Nikon (.NEF), Sony (.ARW), Fujifilm (.RAF).
* Bảng điều khiển RAW: Demosaicing, Exposure compensation, White Balance (Kelvin/Tint), Highlights/Shadows recovery.

#### Sprint 4.3: Xuất File PSD & Xử lý CMYK
* Hiện thực PSD Writer: Xuất file PSD tương thích hoàn toàn để mở lại trên Adobe Photoshop.
* Xử lý chuyển đổi hệ màu CMYK phục vụ in ấn chuyên nghiệp (FOGRA39, US Web Coated SWOP).

#### Sprint 4.4: Hệ thống Plugin (Extensibility Engine)
* Tích hợp **Wasmtime** làm sandbox thực thi an toàn.
* Cung cấp DEP_PHOTO Plugin SDK:
  * Cho phép bên thứ ba viết filter, custom brush, công cụ nhập/xuất tệp bằng Rust, C/C++, hoặc Zig rồi biên dịch ra WebAssembly.
  * Không sợ crash ứng dụng mẹ khi plugin bị lỗi (Sandboxed memory).

#### Sprint 4.5: Batch Processing Engine (CLI)
* Xây dựng binary dòng lệnh `dep_photo_cli`.
* Hỗ trợ tự động hóa: Resize, convert format, áp dụng LUT màu, đóng dấu watermark hàng loạt 10,000 ảnh tận dụng đa nhân CPU (Rayon).

**Tiêu chí nghiệm thu (DoD Phase 4):**
* Nhiếp ảnh gia có thể import file RAW 60MP từ Sony A7R IV, chỉnh sửa và xuất file in ấn chuẩn CMYK.
* Plugin WASM chạy với overhead nhỏ hơn 5% so với native code.

---

### GIAI ĐOẠN 5: TÍCH HỢP AI MODULE (Tháng 15 – 16)
**Mục tiêu:** Tích hợp các mô hình Deep Learning xử lý ảnh cục bộ (On-device AI) với tốc độ cao, tôn trọng quyền riêng tư 100%.

#### Sprint 5.1: Inference Engine Setup
* Tích hợp `ort` (ONNX Runtime Rust binding) hoặc `candle` / `directml` / `coreml` / `wgpu-tensor`.
* Tự động phát hiện phần cứng: Sử dụng NPU / GPU rời để tăng tốc độ suy luận.

#### Sprint 5.2: Tách Nền Thông Minh (Smart Background Removal)
* Tích hợp mô hình segmentation tiên tiến (RMBG / BiRefNet quantization INT8/FP16).
* Tự động tạo Layer Mask độ phân giải cao trong `< 1.5 giây` cho ảnh chân dung và sản phẩm.

#### Sprint 5.3: Phóng To Ảnh AI (Super-Resolution / Upscale)
* Tích hợp mô hình Real-ESRGAN (x2, x4 upscale).
* Xử lý ảnh theo dạng Tiling để tránh tràn VRAM trên các card đồ họa phổ thông (4GB - 6GB VRAM).

#### Sprint 5.4: Xóa Vật Thể Thông Minh (AI Inpainting)
* Tích hợp mô hình LaMa (Large Mask Inpainting).
* Người dùng chỉ cần dùng công cụ Object Removal quét qua chi tiết thừa, hệ thống tự động bù đắp dữ liệu ảnh tự nhiên.

**Tiêu chí nghiệm thu (DoD Phase 5):**
* Toàn bộ tính năng AI chạy **Offline 100%**, không gửi dữ liệu ra ngoài máy người dùng.
* Thời gian tách nền ảnh 4K `< 2 giây` trên GPU tầm trung (RTX 3060 / Apple M-series).

---

### GIAI ĐOẠN 6: ĐÓNG GÓI THƯƠNG MẠI HÓA & PHÁT HÀNH TOÀN CẦU (Tháng 17 – 18)
**Mục tiêu:** Hệ thống cấp phép bản quyền, bộ cài đặt chuẩn signed, tài liệu, website và chiến dịch ra mắt.

#### Sprint 6.1: Hệ thống Bản Quyền & Kích Hoạt (Pro License)
* Mã hóa khóa kích hoạt bằng chuẩn mã hóa bất đối xứng Ed25519.
* Hỗ trợ kích hoạt Offline (Hardware Fingerprint) và kích hoạt trực tuyến qua License Server.
* Tuyệt đối không can thiệp hay cài bloatware/DRM nặng nề làm giảm hiệu năng ứng dụng.

#### Sprint 6.2: Đóng Gói Đa Nền Tảng & Code Signing
* **Windows:** Trình cài đặt NSIS hoặc MSIX, ký số chứng chỉ EV Code Signing (loại bỏ cảnh báo SmartScreen).
* **macOS:** Universal Binary (Apple Silicon M1-M4 & Intel), đóng gói `.dmg`, thực hiện Notarization với Apple Server.
* **Linux:** Phân phối qua Flatpak (Flathub), AppImage, Snap và Arch AUR.

#### Sprint 6.3: Auto-Update & Crash Reporter
* Tích hợp giải pháp cập nhật vi sai an toàn (Delta updates).
* Tùy chọn gửi báo cáo crash ẩn danh để đội ngũ kỹ thuật khắc phục sự cố tức thì.

#### Sprint 6.4: Công Bố Báo Cáo Benchmark & Chiến Dịch Ra Mắt
* Công bố trang web so sánh hiệu năng trực tiếp (Live Performance Benchmark) giữa DEP_PHOTO, Adobe Photoshop 2026, GIMP 3.0 và Affinity Photo 2.
* Phát hành phiên bản thương mại chính thức **DEP_PHOTO Pro 1.0**.

---

## 4. MA TRẬN PHÂN CHIA TRÁCH NHIỆM & TÀI NGUYÊN (RESOURCES & MILESTONES)

| Cột Mốc | Thời Gian | Đầu Ra Then Chốt (Key Deliverables) | Trạng Thái Mục Tiêu |
| :--- | :--- | :--- | :--- |
| **M0: Core Engine** | Tháng 1 | Cấu trúc workspace, wgpu surface, Tile sparse cache, 60 FPS Zoom/Pan | **Alpha Nội Bộ** |
| **M1: Free MVP** | Tháng 4 | Multi-layer, Selection, Basic Brushes, Định dạng `.dpx` V1, Basic Filters | **Private Beta** |
| **M2: Free Release** | Tháng 6 | Curves, Levels, Clipping Mask, Crash Recovery, Packaging hoàn chỉnh | **Public Open Source v1.0** |
| **M3: Pro Core** | Tháng 10 | 16/32-bit Float, PSD Reader, Smart Object, ICC Color Profiles | **Pro Beta (Early Bird)** |
| **M4: Pro Advanced** | Tháng 14 | RAW, CMYK, Healing Brush, WASM Plugin System, CLI Batch | **Pro Release Candidate** |
| **M5: AI Edition** | Tháng 16 | On-device AI Background Removal, Upscale, Inpainting | **Pro AI Update** |
| **M6: Global Launch** | Tháng 18 | Installer đa nền tảng, Code Signing, Thanh toán, Benchmark toàn cầu | **Commercial Launch 1.0** |

---

## 5. MA TRẬN QUẢN TRỊ RỦI RO KỸ THUẬT (RISK & MITIGATION)

| Rủi Ro Kỹ Thuật | Mức Độ | Tác Động Tiềm Tàng | Chiến Lược Phòng Ngừa & Xử Lý |
| :--- | :---: | :--- | :--- |
| **Tràn VRAM/RAM khi mở ảnh 100MP nhiều layer** | Cao | Crash ứng dụng do Out-Of-Memory (OOM) | Sử dụng **LRU Tile Cache**, chỉ giữ trên VRAM các tile trong Viewport + buffer lân cận; các tile khuất được nén lưu trên RAM hoặc swap ra SSD scratch. |
| **Độ trễ nét vẽ (Input Lag) trên màn hình 4K/144Hz** | Cao | Nét vẽ giật cục, trải nghiệm kém | Tách biệt hoàn toàn luồng nhận diện sự kiện chuột (Raw Input Thread) với luồng Render; nội suy Spline các điểm tọa độ và blit trực tiếp vào Texture Tile. |
| **Tương thích driver GPU khác nhau (AMD/Nvidia/Intel/Apple Metal)** | Cao | Shader biên dịch lỗi hoặc sai lệch kết quả màu | Khai thác lớp trừu tượng `wgpu`, kiểm thử tự động trên nhiều GPU backends (Vulkan, Metal, DirectX 12); có sẵn **CPU SIMD Fallback** cho các máy không có GPU rời. |
| **Cấu trúc file PSD quá phức tạp và nhiều thông số ẩn** | Trung bình | Đọc lỗi các file PSD đặc thù | Triển khai Fuzz Testing với tập dữ liệu hàng nghìn file PSD công khai; ưu tiên đọc đúng Raster Data & Masks trước khi parse các effect nâng cao. |
| **Kích thước mô hình AI làm phình dung lượng bộ cài** | Trung bình | Tải ứng dụng nặng nề (>1GB) | Tách module AI thành **Addon tải theo nhu cầu (Download-on-demand)**; tối ưu lượng tử hóa mô hình sang INT8/FP16 để kích thước mỗi model `< 40MB`. |

---

## 6. QUY TẮC PHÁT TRIỂN & CHUẨN MỰC KỸ THUẬT (DEVELOPMENT PROTOCOLS)

1. **Hiệu Năng Là Tính Năng (Performance-First):** Bất kỳ PR (Pull Request) nào làm tăng thời gian render quá 5% trên bộ benchmark của `criterion` sẽ bị từ chối tự động.
2. **Không Snapshot Toàn Document:** Mọi thao tác chỉnh sửa chỉ được tác động lên các Tile bị ảnh hưởng (Dirty Tiles). Lịch sử Undo chỉ lưu Tile Diff.
3. **Độc Lập Tuyệt Đối Giữa Engine & UI:** Toàn bộ logic tính toán đồ họa nằm trong `dep_photo_core` và `dep_photo_engine`. Crate giao diện `dep_photo_app` (`egui`) chỉ là một lớp hiển thị (View Layer) mỏng, có thể thay thế bằng giao diện khác trong tương lai nếu cần.
4. **Kiểm Thử Toàn Diện (Zero Panic Policy):** Không sử dụng `.unwrap()` hoặc `.expect()` trên luồng dữ liệu sản phẩm; bắt buộc xử lý lỗi qua `Result<T, DepError>` với `thiserror`.
5. **Mọi Thay Đổi Kiến Trúc Bắt Buộc Có ADR:** Tài liệu hóa các quyết định kỹ thuật trong thư mục `docs/adr/`.
