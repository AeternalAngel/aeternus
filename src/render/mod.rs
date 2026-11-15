mod settings;
pub mod trail;
mod ui;

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use bytemuck::{Pod, Zeroable};
use cosmic_text::{
    Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Style, SwashCache, SwashContent, Weight,
};
use wgpu::util::DeviceExt;
use winit::window::Window;

use crate::cell::{flags, Cell};
use crate::colors::Palette;
use crate::config::{Config, TextComposition};
use crate::log;
use crate::term::{CursorShape, Term, WIDE_SPACER};

pub use settings::SettingsOutcome;
use settings::SettingsState;
use trail::{Trail, TrailConfig};
pub use ui::Action;

const ATLAS_SIZE: u32 = 2048;
const WHITE_UV: [f32; 2] = [1.0 / ATLAS_SIZE as f32, 1.0 / ATLAS_SIZE as f32];
const SENTINEL: [u8; 3] = [1, 2, 3];
const ART_INK: [u8; 3] = [2, 4, 6];
const BELL_FLASH: Duration = Duration::from_millis(120);

pub struct View {
    pub focused: bool,
    pub blink_on: bool,
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    pos: [f32; 2],
    size: [f32; 2],
    uv_pos: [f32; 2],
    uv_size: [f32; 2],
    color: [f32; 4],
    color_glyph: f32,
}
impl Instance {
    const ATTRS: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
        0 => Float32x2, 1 => Float32x2, 2 => Float32x2, 3 => Float32x2, 4 => Float32x4, 5 => Float32
    ];
    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Instance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &Self::ATTRS,
        }
    }
    fn rect(x: f32, y: f32, w: f32, h: f32, rgb: [u8; 3]) -> Self {
        Self::rect_alpha(x, y, w, h, rgb, 1.0)
    }
    fn rect_alpha(x: f32, y: f32, w: f32, h: f32, rgb: [u8; 3], a: f32) -> Self {
        let mut color = color4(rgb);
        color[3] = a;
        Self { pos: [x, y], size: [w, h], uv_pos: WHITE_UV, uv_size: [0.0, 0.0], color, color_glyph: 0.0 }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PolyVertex {
    pos: [f32; 2],
    color: [f32; 4],
}
impl PolyVertex {
    const ATTRS: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4];
    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<PolyVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}
fn color4(c: [u8; 3]) -> [f32; 4] {
    [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, 1.0]
}
fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    [f(a[0], b[0]), f(a[1], b[1]), f(a[2], b[2])]
}
fn outline(out: &mut Vec<Instance>, x: f32, y: f32, w: f32, h: f32, thick: f32, col: [u8; 3]) {
    out.push(Instance::rect(x, y, w, thick, col));
    out.push(Instance::rect(x, y + h - thick, w, thick, col));
    out.push(Instance::rect(x, y, thick, h, col));
    out.push(Instance::rect(x + w - thick, y, thick, h, col));
}
fn square_outline(out: &mut Vec<Instance>, x: f32, y: f32, s: f32, thick: f32, col: [u8; 3]) {
    outline(out, x, y, s, s, thick, col);
}
fn x_cross(out: &mut Vec<Instance>, cx: f32, cy: f32, s: f32, thick: f32, col: [u8; 3]) {
    let steps = (((s - thick) / thick).ceil() as i32).max(1);
    let h = s / 2.0;
    for i in 0..=steps {
        let t = i as f32 * thick;
        out.push(Instance::rect(cx - h + t, cy - h + t, thick, thick, col));
        out.push(Instance::rect(cx - h + t, cy + h - t - thick, thick, thick, col));
    }
}
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [u8; 3] {
    let hh = h.rem_euclid(360.0) / 60.0;
    let i = hh.floor() as i32;
    let f = hh - hh.floor();
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    let (r, g, b) = match i.rem_euclid(6) {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    let c = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    [c(r), c(g), c(b)]
}
fn rgb_to_hsv(c: [u8; 3]) -> (f32, f32, f32) {
    let r = c[0] as f32 / 255.0;
    let g = c[1] as f32 / 255.0;
    let b = c[2] as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d <= 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d * 60.0).rem_euclid(360.0)
    } else if max == g {
        (2.0 + (b - r) / d) * 60.0
    } else {
        (4.0 + (r - g) / d) * 60.0
    };
    let s = if max <= 0.0 { 0.0 } else { d / max };
    (h, s, max)
}
fn hex_str(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}
fn luma(c: [u8; 3]) -> u8 {
    ((c[0] as u32 * 299 + c[1] as u32 * 587 + c[2] as u32 * 114) / 1000) as u8
}
fn system_monospace() -> &'static str {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(|| {
        std::process::Command::new("fc-match")
            .args(["-f", "%{family[0]}", "monospace"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "DejaVu Sans Mono".to_string())
    })
}
fn resolve_family(name: &str) -> String {
    if name.trim().eq_ignore_ascii_case("monospace") {
        system_monospace().to_string()
    } else {
        name.trim().to_string()
    }
}
fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}
fn is_private_use(c: char) -> bool {
    matches!(c as u32, 0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD)
}
fn is_powerline(c: char) -> bool {
    matches!(c as u32, 0xE0A0..=0xE0D7)
}
fn is_symbol(c: char) -> bool {
    is_private_use(c)
        || matches!(c as u32, 0x2190..=0x21FF | 0x2300..=0x23FF | 0x2460..=0x24FF | 0x25A0..=0x27BF | 0x2B00..=0x2BFF)
}
fn family_installed(fs: &FontSystem, name: &str) -> bool {
    fs.db().faces().any(|f| f.families.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)))
}
#[derive(Clone, Copy)]
struct GlyphInfo {
    uv_pos: [f32; 2],
    uv_size: [f32; 2],
    off: [f32; 2],
    size: [f32; 2],
    is_color: bool,
}
struct Atlas {
    texture: wgpu::Texture,
    x: u32,
    y: u32,
    row_h: u32,
}
impl Atlas {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("atlas"),
            size: wgpu::Extent3d { width: ATLAS_SIZE, height: ATLAS_SIZE, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let atlas = Self { texture, x: 3, y: 0, row_h: 2 };
        atlas.write(queue, 0, 0, 2, 2, &[255; 16]);
        atlas
    }
    fn reset(&mut self) {
        self.x = 3;
        self.y = 0;
        self.row_h = 2;
    }
    fn alloc(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        if w > ATLAS_SIZE || h > ATLAS_SIZE {
            return None;
        }
        if self.x + w > ATLAS_SIZE {
            self.x = 0;
            self.y += self.row_h + 1;
            self.row_h = 0;
        }
        if self.y + h > ATLAS_SIZE {
            return None;
        }
        let pos = (self.x, self.y);
        self.x += w + 1;
        self.row_h = self.row_h.max(h);
        Some(pos)
    }
    fn write(&self, queue: &wgpu::Queue, x: u32, y: u32, w: u32, h: u32, data: &[u8]) {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }
}
struct Pipelines {
    main: wgpu::RenderPipeline,
    poly: wgpu::RenderPipeline,
}
pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    raw_pipelines: Pipelines,
    linear_pipelines: Option<(wgpu::TextureFormat, Pipelines)>,
    composition: TextComposition,
    bind_group: wgpu::BindGroup,
    globals: wgpu::Buffer,
    inst_buf: wgpu::Buffer,
    inst_cap: usize,
    poly_buf: wgpu::Buffer,
    atlas: Atlas,
    glyphs: HashMap<(char, bool, bool), Option<GlyphInfo>>,
    cluster_glyphs: HashMap<(String, bool, bool), Vec<GlyphInfo>>,
    fitted: HashMap<(char, bool, bool, u8), Option<GlyphInfo>>,
    ascii: Vec<Option<Option<GlyphInfo>>>,
    font_system: FontSystem,
    swash: SwashCache,
    font_family: String,
    base_font_size: f32,
    line_height: f32,
    font_size: f32,
    scale: f32,
    zoom: f32,
    pad: f32,
    window_padding: f32,
    palette: Palette,
    pub cell_w: f32,
    pub cell_h: f32,
    bg_inst: Vec<Instance>,
    fg_inst: Vec<Instance>,
    poly: Vec<PolyVertex>,
    effective_bg: [u8; 3],
    atlas_reset: bool,
    toolbar: bool,
    toolbar_h: f32,
    title_rects: Vec<(f32, f32)>,
    open_menu: Option<usize>,
    hover_title: Option<usize>,
    hover_item: Option<usize>,
    settings: Option<SettingsState>,
    shortcuts: HashMap<Action, String>,
    tabs: Vec<(u64, String)>,
    active_tab: u64,
    tab_rects: Vec<(f32, f32, u64)>,
    close_rects: Vec<(f32, f32, u64)>,
    tab_scroll: usize,
    tab_prev_rect: Option<(f32, f32)>,
    tab_next_rect: Option<(f32, f32)>,
    new_tab_rect: Option<(f32, f32)>,
    gear_rect: Option<(f32, f32)>,
    hover_tab: Option<u64>,
    hover_close: Option<u64>,
    hover_new: bool,
    hover_prev: bool,
    hover_next: bool,
    hover_gear: bool,
    opacity: f32,
    alpha_modes: Vec<wgpu::CompositeAlphaMode>,
    trail: Trail,
    trail_cfg: TrailConfig,
    trail_color: Option<[u8; 3]>,
    flash_until: Option<Instant>,
}
impl Renderer {
    pub fn new(window: Arc<Window>, cfg: &Config) -> Result<Self, String> {
        let size = window.inner_size();
        let scale = window.scale_factor() as f32;
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle_from_env(
            Box::new(window.clone()),
        ));
        let surface = instance
            .create_surface(window)
            .map_err(|e| format!("could not create a rendering surface: {e}"))?;
        let mut adapter = None;
        for fallback in [false, true] {
            let opts = wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: fallback,
                ..Default::default()
            };
            if let Ok(a) = pollster::block_on(instance.request_adapter(&opts)) {
                adapter = Some(a);
                break;
            }
        }
        let adapter = adapter.ok_or_else(|| {
            "no compatible GPU adapter found (a Vulkan or OpenGL ES 3 driver is required)".to_string()
        })?;
        let info = adapter.get_info();
        log::info(&format!("aeternus: using {} ({:?})", info.name, info.backend));
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .map_err(|e| format!("could not open the GPU device: {e}"))?;
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or_else(|| "the GPU does not support this window surface".to_string())?;
        let caps = surface.get_capabilities(&adapter);
        let preferred = [
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureFormat::Rgb10a2Unorm,
        ];
        if let Some(f) = preferred.into_iter().find(|f| caps.formats.contains(f)) {
            config.format = f;
        } else {
            log::error(&format!(
                "aeternus: no 8-bit surface format available, colors may look off ({:?})",
                caps.formats
            ));
        }
        config.color_space = wgpu::SurfaceColorSpace::Auto;
        let alpha_modes = caps.alpha_modes.clone();
        config.alpha_mode = pick_alpha_mode(&alpha_modes, cfg.window.opacity);
        log::info(&format!(
            "aeternus: surface format {:?}, alpha mode {:?} (available {:?})",
            config.format, config.alpha_mode, caps.alpha_modes
        ));
        config.present_mode = wgpu::PresentMode::Fifo;
        config.desired_maximum_frame_latency = 2;
        let srgb_format = match config.format {
            wgpu::TextureFormat::Bgra8Unorm => Some(wgpu::TextureFormat::Bgra8UnormSrgb),
            wgpu::TextureFormat::Rgba8Unorm => Some(wgpu::TextureFormat::Rgba8UnormSrgb),
            _ => None,
        };
        config.view_formats = srgb_format.into_iter().collect();
        surface.configure(&device, &config);
        let format = config.format;
        let atlas = Atlas::new(&device, &queue);
        let view = atlas.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let globals = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("globals"),
            contents: bytemuck::cast_slice(&[0.0f32; 8]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let pl_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let target = |format: wgpu::TextureFormat| {
            [Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::SrcAlpha,
                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })]
        };
        let make_pipeline = |vs: &str,
                             fs: &str,
                             layout: wgpu::VertexBufferLayout<'static>,
                             topology: wgpu::PrimitiveTopology,
                             format: wgpu::TextureFormat| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
                layout: Some(&pl_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers: &[Some(layout)],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &target(format),
                }),
                primitive: wgpu::PrimitiveState { topology, ..Default::default() },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipelines = |format: wgpu::TextureFormat| Pipelines {
            main: make_pipeline(
                "vs_main",
                "fs_main",
                Instance::layout(),
                wgpu::PrimitiveTopology::TriangleStrip,
                format,
            ),
            poly: make_pipeline(
                "vs_poly",
                "fs_poly",
                PolyVertex::layout(),
                wgpu::PrimitiveTopology::TriangleList,
                format,
            ),
        };
        let raw_pipelines = pipelines(format);
        let linear_pipelines = srgb_format.map(|f| (f, pipelines(f)));
        let inst_cap = 16384;
        let inst_buf = Self::make_inst_buf(&device, inst_cap);
        let poly_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("poly"),
            size: (64 * std::mem::size_of::<PolyVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut font_system = FontSystem::new();
        font_system.db_mut().set_monospace_family(system_monospace());
        let font_family = resolve_family(&cfg.font.family);
        if !family_installed(&font_system, &font_family) {
            log::error(&format!("aeternus: font {font_family:?} not found, using a fallback font"));
        }
        let font_size = cfg.font.size * scale;
        let cell_h = (font_size * cfg.font.line_height).round().max(1.0);
        let cell_w = measure_cell_width(&mut font_system, &font_family, font_size, cell_h).round().max(1.0);
        let mut r = Self {
            surface,
            device,
            queue,
            config,
            raw_pipelines,
            linear_pipelines,
            composition: cfg.text_composition,
            bind_group,
            globals,
            inst_buf,
            inst_cap,
            poly_buf,
            atlas,
            glyphs: HashMap::new(),
            cluster_glyphs: HashMap::new(),
            fitted: HashMap::new(),
            ascii: vec![None; 128 * 4],
            font_system,
            swash: SwashCache::new(),
            font_family,
            base_font_size: cfg.font.size,
            line_height: cfg.font.line_height,
            font_size,
            scale,
            zoom: 1.0,
            pad: (cfg.window.padding * scale).round(),
            window_padding: cfg.window.padding,
            palette: cfg.palette.clone(),
            cell_w,
            cell_h,
            bg_inst: Vec::new(),
            fg_inst: Vec::new(),
            poly: Vec::new(),
            effective_bg: cfg.palette.bg,
            atlas_reset: false,
            toolbar: cfg.window.toolbar,
            toolbar_h: if cfg.window.toolbar { (36.0 * scale).round() } else { 0.0 },
            title_rects: Vec::new(),
            open_menu: None,
            hover_title: None,
            hover_item: None,
            settings: None,
            shortcuts: HashMap::new(),
            tabs: Vec::new(),
            active_tab: 0,
            tab_rects: Vec::new(),
            close_rects: Vec::new(),
            tab_scroll: 0,
            tab_prev_rect: None,
            tab_next_rect: None,
            new_tab_rect: None,
            gear_rect: None,
            hover_tab: None,
            hover_close: None,
            hover_new: false,
            hover_prev: false,
            hover_next: false,
            hover_gear: false,
            opacity: cfg.window.opacity,
            alpha_modes,
            trail: Trail::default(),
            trail_cfg: trail_config(cfg),
            trail_color: cfg.trail_color,
            flash_until: None,
        };
        r.set_shortcuts(cfg);
        r.write_globals();
        Ok(r)
    }
    fn set_shortcuts(&mut self, cfg: &Config) {
        self.shortcuts.clear();
        for (_, items) in ui::MENUS.iter() {
            for it in items.iter() {
                let Some(a) = it.action else { continue };
                if let Some(label) = a.key_action().and_then(|k| cfg.keymap.label(k)) {
                    self.shortcuts.insert(a, label);
                }
            }
        }
    }
    fn make_inst_buf(device: &wgpu::Device, cap: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instances"),
            size: (cap * std::mem::size_of::<Instance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }
    fn linear(&self) -> bool {
        self.composition.linear && self.linear_pipelines.is_some()
    }
    fn write_globals(&self) {
        let c = self.composition;
        let data = [
            self.config.width as f32,
            self.config.height as f32,
            c.gamma,
            c.contrast,
            if self.linear() { 1.0 } else { 0.0 },
            0.0,
            0.0,
            0.0,
        ];
        self.queue.write_buffer(&self.globals, 0, bytemuck::cast_slice(&data));
    }
    fn bg_alpha(&self) -> f32 {
        match self.config.alpha_mode {
            wgpu::CompositeAlphaMode::PreMultiplied | wgpu::CompositeAlphaMode::PostMultiplied => {
                self.opacity
            }
            _ => 1.0,
        }
    }
    pub fn pad(&self) -> f32 {
        self.pad
    }
    pub fn cell_px(&self) -> (u32, u32) {
        (self.cell_w as u32, self.cell_h as u32)
    }
    pub fn about_accent(&self) -> [u8; 3] {
        let (h, s, v) = rgb_to_hsv(self.effective_bg);
        if luma(self.effective_bg) >= 128 {
            hsv_to_rgb(h, s * 0.55, 0.12)
        } else {
            hsv_to_rgb(h, s * 0.55, (v + 0.45).min(1.0))
        }
    }
    fn art_ink(&self) -> [u8; 3] {
        if luma(self.effective_bg) >= 128 {
            [0, 0, 0]
        } else {
            [255, 255, 255]
        }
    }
    fn push_text(&mut self, fgs: &mut Vec<Instance>, text: &str, x: f32, y: f32, color: [u8; 3]) {
        let cw = self.cell_w;
        for (i, c) in text.chars().enumerate() {
            if let Some(g) = self.glyph(c, false, false) {
                fgs.push(Instance {
                    pos: [x + i as f32 * cw + g.off[0], y + g.off[1]],
                    size: g.size,
                    uv_pos: g.uv_pos,
                    uv_size: g.uv_size,
                    color: color4(color),
                    color_glyph: 0.0,
                });
            }
        }
    }
    pub fn zoom(&self) -> f32 {
        self.zoom
    }
    fn recompute_metrics(&mut self) {
        self.font_size = self.base_font_size * self.scale * self.zoom;
        self.cell_h = (self.font_size * self.line_height).round().max(1.0);
        self.cell_w =
            measure_cell_width(&mut self.font_system, &self.font_family, self.font_size, self.cell_h)
                .round()
                .max(1.0);
        self.reset_atlas();
        self.trail.reset();
    }
    pub fn set_zoom(&mut self, z: f32) {
        self.zoom = (z.clamp(0.5, 4.0) * 20.0).round() / 20.0;
        self.recompute_metrics();
    }
    pub fn apply_config(&mut self, cfg: &Config) {
        let family = resolve_family(&cfg.font.family);
        if family != self.font_family && !family_installed(&self.font_system, &family) {
            log::error(&format!("aeternus: font {family:?} not found, using a fallback font"));
        }
        self.font_family = family;
        self.base_font_size = cfg.font.size;
        self.line_height = cfg.font.line_height;
        self.window_padding = cfg.window.padding;
        self.pad = (self.window_padding * self.scale).round();
        self.toolbar = cfg.window.toolbar;
        self.toolbar_h = if cfg.window.toolbar { (36.0 * self.scale).round() } else { 0.0 };
        self.palette = cfg.palette.clone();
        if cfg.window.opacity != self.opacity {
            self.opacity = cfg.window.opacity;
            let mode = pick_alpha_mode(&self.alpha_modes, self.opacity);
            if self.config.alpha_mode == wgpu::CompositeAlphaMode::Opaque && mode != self.config.alpha_mode {
                self.config.alpha_mode = mode;
                self.surface.configure(&self.device, &self.config);
            }
        }
        self.trail_cfg = trail_config(cfg);
        self.trail_color = cfg.trail_color;
        self.composition = cfg.text_composition;
        self.write_globals();
        self.set_shortcuts(cfg);
        self.recompute_metrics();
    }
    pub fn set_scale(&mut self, scale: f32) {
        if scale == self.scale {
            return;
        }
        self.scale = scale;
        self.pad = (self.window_padding * self.scale).round();
        self.toolbar_h = if self.toolbar { (36.0 * self.scale).round() } else { 0.0 };
        self.recompute_metrics();
    }
    pub fn grid_size(&self) -> (usize, usize) {
        let w = self.config.width as f32 - 2.0 * self.pad;
        let h = self.config.height as f32 - 2.0 * self.pad - self.top_inset();
        let cols = (w / self.cell_w) as usize;
        let rows = (h / self.cell_h) as usize;
        (cols.max(2), rows.max(1))
    }
    pub fn cell_at(&self, x: f64, y: f64) -> (usize, usize) {
        let (cols, rows) = self.grid_size();
        let c = ((x as f32 - self.pad) / self.cell_w).floor().max(0.0) as usize;
        let r = ((y as f32 - self.pad - self.top_inset()) / self.cell_h).floor().max(0.0) as usize;
        (r.min(rows - 1), c.min(cols - 1))
    }
    pub fn resize(&mut self, w: u32, h: u32) {
        self.config.width = w.max(1);
        self.config.height = h.max(1);
        self.surface.configure(&self.device, &self.config);
        self.write_globals();
        self.trail.reset();
    }
    pub fn reset_trail(&mut self) {
        self.trail.reset();
    }
    pub fn trail_animating(&self) -> bool {
        self.trail.animating()
    }
    pub fn flash(&mut self, now: Instant) {
        self.flash_until = Some(now + BELL_FLASH);
    }
    pub fn flash_deadline(&self) -> Option<Instant> {
        self.flash_until
    }
    fn glyph(&mut self, ch: char, bold: bool, italic: bool) -> Option<GlyphInfo> {
        if (ch as u32) < 128 {
            let idx = ((ch as usize) << 2) | ((bold as usize) << 1) | italic as usize;
            if let Some(g) = self.ascii[idx] {
                return g;
            }
            let g = self.rasterize(&ch.to_string(), bold, italic, self.font_size).into_iter().next();
            self.ascii[idx] = Some(g);
            return g;
        }
        let key = (ch, bold, italic);
        if let Some(g) = self.glyphs.get(&key) {
            return *g;
        }
        let g = self.rasterize(&ch.to_string(), bold, italic, self.font_size).into_iter().next();
        self.glyphs.insert(key, g);
        g
    }
    fn cluster(&mut self, text: &str, bold: bool, italic: bool) -> Vec<GlyphInfo> {
        let key = (text.to_string(), bold, italic);
        if let Some(g) = self.cluster_glyphs.get(&key) {
            return g.clone();
        }
        let g = self.rasterize(text, bold, italic, self.font_size);
        self.cluster_glyphs.insert(key, g.clone());
        g
    }
    fn reset_atlas(&mut self) {
        self.atlas.reset();
        self.glyphs.clear();
        self.cluster_glyphs.clear();
        self.fitted.clear();
        self.ascii.fill(None);
        self.atlas_reset = true;
    }
    fn glyph_in_cells(&mut self, ch: char, bold: bool, italic: bool, cells: u8) -> Option<GlyphInfo> {
        let natural = self.glyph(ch, bold, italic)?;
        if (ch as u32) < 128 || is_powerline(ch) {
            return Some(natural);
        }
        let key = (ch, bold, italic, cells);
        if let Some(g) = self.fitted.get(&key) {
            return *g;
        }
        let avail_w = self.cell_w * cells as f32;
        let avail_h = self.cell_h;
        let [x, y] = natural.off;
        let [w, h] = natural.size;
        let overflow = if is_symbol(ch) {
            x < -1.0 || y < -1.0 || x + w > avail_w + 1.0 || y + h > avail_h + 1.0
        } else {
            w > avail_w * 1.15 || h > avail_h * 1.15
        };
        let fitted = if overflow {
            let scale = (avail_w / w).min(avail_h / h).min(1.0);
            let size = (self.font_size * scale).max(1.0);
            let mut g =
                self.rasterize(&ch.to_string(), bold, italic, size).into_iter().next().unwrap_or(natural);
            g.off = [((avail_w - g.size[0]) / 2.0).round(), ((avail_h - g.size[1]) / 2.0).round()];
            Some(g)
        } else {
            Some(natural)
        };
        self.fitted.insert(key, fitted);
        fitted
    }
    fn rasterize(&mut self, text: &str, bold: bool, italic: bool, size: f32) -> Vec<GlyphInfo> {
        let attrs = Attrs::new()
            .family(Family::Name(&self.font_family))
            .weight(if bold { Weight::BOLD } else { Weight::NORMAL })
            .style(if italic { Style::Italic } else { Style::Normal });
        let mut buf = Buffer::new(&mut self.font_system, Metrics::new(size, self.cell_h));
        buf.set_text(text, &attrs, Shaping::Advanced, None);
        buf.shape_until_scroll(&mut self.font_system, false);
        let mut phys = Vec::new();
        let mut line_y = 0.0;
        if let Some(run) = buf.layout_runs().next() {
            line_y = run.line_y;
            phys.extend(run.glyphs.iter().map(|g| g.physical((0.0, 0.0), 1.0)));
        }
        let mut out = Vec::new();
        for p in phys {
            let Some(image) = self.swash.get_image(&mut self.font_system, p.cache_key).as_ref() else {
                continue;
            };
            let pl = image.placement;
            if pl.width == 0 || pl.height == 0 {
                continue;
            }
            let (is_color, rgba): (bool, Vec<u8>) = match image.content {
                SwashContent::Mask => (false, image.data.iter().flat_map(|&a| [a, 0, 0, 0]).collect()),
                SwashContent::Color => (true, image.data.clone()),
                _ => continue,
            };
            let pos = match self.atlas.alloc(pl.width, pl.height) {
                Some(pos) => pos,
                None => {
                    self.reset_atlas();
                    match self.atlas.alloc(pl.width, pl.height) {
                        Some(pos) => pos,
                        None => continue,
                    }
                }
            };
            self.atlas.write(&self.queue, pos.0, pos.1, pl.width, pl.height, &rgba);
            let a = ATLAS_SIZE as f32;
            out.push(GlyphInfo {
                uv_pos: [pos.0 as f32 / a, pos.1 as f32 / a],
                uv_size: [pl.width as f32 / a, pl.height as f32 / a],
                off: [(p.x + pl.left) as f32, (line_y as i32 + p.y - pl.top) as f32],
                size: [pl.width as f32, pl.height as f32],
                is_color,
            });
        }
        out
    }
    pub fn build(&mut self, term: &Term, view: &View) {
        self.atlas_reset = false;
        self.build_pass(term, view);
        if self.atlas_reset {
            self.atlas_reset = false;
            self.build_pass(term, view);
        }
    }
    fn build_pass(&mut self, term: &Term, view: &View) {
        let mut bgs = std::mem::take(&mut self.bg_inst);
        let mut fgs = std::mem::take(&mut self.fg_inst);
        bgs.clear();
        fgs.clear();
        self.poly.clear();
        let mut pal = self.palette.clone();
        if let Some(c) = term.fg_override {
            pal.fg = c;
        }
        if let Some(c) = term.bg_override {
            pal.bg = c;
        }
        if let Some(c) = term.cursor_override {
            pal.cursor = c;
        }
        for (&i, &c) in &term.ansi_override {
            if (i as usize) < pal.ansi.len() {
                pal.ansi[i as usize] = c;
            }
        }
        self.effective_bg = pal.bg;
        let accent = self.about_accent();
        let ink = self.art_ink();
        let (cw, ch, pad) = (self.cell_w, self.cell_h, self.pad);
        let thick = self.scale.round().max(1.0);
        let cols = term.cols();
        self.push_toolbar(&mut bgs, &mut fgs, pal.bg, pal.fg, thick);
        let top = pad + self.top_inset();
        let sel = term.selection_bounds();
        let cursor_shown = term.cursor_visible && term.display_offset == 0;
        let cursor_vis = cursor_shown && (!view.focused || !term.cursor_blink || view.blink_on);
        let block = view.focused && term.cursor_shape == CursorShape::Block;
        for r in 0..term.rows() {
            let y = top + r as f32 * ch;
            let abs = term.abs_line(r);
            let span = sel.and_then(|(a, b)| {
                if abs < a.line || abs > b.line {
                    None
                } else {
                    let s = if abs == a.line { a.col } else { 0 };
                    let e = if abs == b.line { b.col } else { usize::MAX };
                    Some((s, e))
                }
            });
            let row = term.visible_row(r);
            for c in 0..cols {
                let cell = row.get(c).copied().unwrap_or(Cell::BLANK);
                let selected = span.is_some_and(|(s, e)| c >= s && c <= e);
                let at_cursor = cursor_vis && r == term.cursor.row && c == term.cursor.col;
                if cell == Cell::BLANK && !selected && !at_cursor {
                    continue;
                }
                let is_spacer = cell.flags & WIDE_SPACER != 0;
                if is_spacer && c > 0 && !at_cursor {
                    continue;
                }
                let wide = !is_spacer && row.get(c + 1).is_some_and(|n| n.flags & WIDE_SPACER != 0);
                let w = if wide { cw * 2.0 } else { cw };
                let x = pad + c as f32 * cw;
                let block_cursor = at_cursor && block;
                let mut fg = pal.rgb(cell.fg, pal.fg);
                let mut bg = pal.rgb(cell.bg, pal.bg);
                for v in [&mut fg, &mut bg] {
                    if *v == SENTINEL {
                        *v = accent;
                    } else if *v == ART_INK {
                        *v = ink;
                    }
                }
                if cell.flags & flags::INVERSE != 0 {
                    std::mem::swap(&mut fg, &mut bg);
                }
                if block_cursor {
                    fg = pal.cursor_text.unwrap_or(bg);
                    bg = pal.cursor;
                } else if selected {
                    bg = pal.selection;
                    if let Some(c) = pal.selection_fg {
                        fg = c;
                    }
                }
                if cell.flags & flags::DIM != 0 {
                    fg = fg.map(|v| (v as f32 * 0.6) as u8);
                }
                if bg != pal.bg {
                    bgs.push(Instance::rect(x, y, w, ch, bg));
                }
                if cell.ch != ' ' && cell.flags & flags::HIDDEN == 0 {
                    let bold = cell.flags & flags::BOLD != 0;
                    let italic = cell.flags & flags::ITALIC != 0;
                    let glyphs: Vec<GlyphInfo> = match term.cluster(&cell) {
                        Some(extra) => {
                            let mut s = String::with_capacity(8);
                            s.push(cell.ch);
                            s.push_str(extra);
                            self.cluster(&s, bold, italic)
                        }
                        None => {
                            let next_free = row.get(c + 1).is_some_and(|n| {
                                n.ch == ' ' && n.flags & WIDE_SPACER == 0 && term.cluster(n).is_none()
                            });
                            let cells = if wide || (is_private_use(cell.ch) && next_free) { 2 } else { 1 };
                            self.glyph_in_cells(cell.ch, bold, italic, cells).into_iter().collect()
                        }
                    };
                    for g in glyphs {
                        fgs.push(Instance {
                            pos: [x + g.off[0], y + g.off[1]],
                            size: g.size,
                            uv_pos: g.uv_pos,
                            uv_size: g.uv_size,
                            color: color4(fg),
                            color_glyph: if g.is_color { 1.0 } else { 0.0 },
                        });
                    }
                }
                if cell.flags & flags::UNDERLINE != 0 || term.link(&cell).is_some() {
                    fgs.push(Instance::rect(x, y + ch - thick * 2.0, w, thick, fg));
                }
                if cell.flags & flags::STRIKE != 0 {
                    fgs.push(Instance::rect(x, y + (ch / 2.0).floor(), w, thick, fg));
                }
                if at_cursor && !block_cursor {
                    if !view.focused {
                        outline(&mut fgs, x, y, w, ch, thick, pal.cursor);
                    } else if term.cursor_shape == CursorShape::Underline {
                        fgs.push(Instance::rect(x, y + ch - thick * 2.0, w, thick * 2.0, pal.cursor));
                    } else {
                        fgs.push(Instance::rect(x, y, thick * 2.0, ch, pal.cursor));
                    }
                }
            }
        }
        let now = Instant::now();
        let target = if cursor_shown && self.settings.is_none() {
            let (r, c) = (term.cursor.row, term.cursor.col);
            let wide = term.visible_row(r).get(c + 1).is_some_and(|n| n.flags & WIDE_SPACER != 0);
            let w = if wide { cw * 2.0 } else { cw };
            let x = pad + c as f32 * cw;
            let y = top + r as f32 * ch;
            let q = match (view.focused, term.cursor_shape) {
                (true, CursorShape::Bar) => trail::rect_quad(x, y, thick * 2.0, ch),
                (true, CursorShape::Underline) => trail::rect_quad(x, y + ch - thick * 2.0, w, thick * 2.0),
                _ => trail::rect_quad(x, y, w, ch),
            };
            Some((q, (r, c)))
        } else {
            None
        };
        self.trail.update(target, &self.trail_cfg, (cw, ch), now);
        if let Some(q) = self.trail.quad() {
            let col = color4(self.trail_color.unwrap_or(pal.cursor));
            for p in trail::hull_triangles(&q) {
                self.poly.push(PolyVertex { pos: p, color: col });
            }
        }
        if let Some(m) = self.open_menu {
            self.push_menu_panel(&mut fgs, m, pal.bg, pal.fg, thick);
        }
        if self.settings.is_some() {
            self.push_settings(&mut fgs, pal.bg, pal.fg, thick);
        }
        match self.flash_until {
            Some(t) if now < t => {
                let w = self.config.width as f32;
                let h = self.config.height as f32;
                fgs.push(Instance::rect_alpha(0.0, 0.0, w, h, pal.fg, 0.12));
            }
            Some(_) => self.flash_until = None,
            None => {}
        }
        self.bg_inst = bgs;
        self.fg_inst = fgs;
    }
    pub fn draw(&mut self) {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) => f,
            wgpu::CurrentSurfaceTexture::Suboptimal(f) => {
                drop(f);
                self.surface.configure(&self.device, &self.config);
                return;
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            _ => return,
        };
        let linear = self.linear();
        let view_format = if linear { self.linear_pipelines.as_ref().map(|(f, _)| *f) } else { None };
        let tex_view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor { format: view_format, ..Default::default() });
        let n_bg = self.bg_inst.len();
        let n = n_bg + self.fg_inst.len();
        if n > self.inst_cap {
            self.inst_cap = n.next_power_of_two();
            self.inst_buf = Self::make_inst_buf(&self.device, self.inst_cap);
        }
        let isz = std::mem::size_of::<Instance>() as u64;
        self.queue.write_buffer(&self.inst_buf, 0, bytemuck::cast_slice(&self.bg_inst));
        self.queue.write_buffer(&self.inst_buf, n_bg as u64 * isz, bytemuck::cast_slice(&self.fg_inst));
        let n_poly = self.poly.len().min(64);
        if n_poly > 0 {
            self.queue.write_buffer(&self.poly_buf, 0, bytemuck::cast_slice(&self.poly[..n_poly]));
        }
        let bg = self.effective_bg;
        let a = self.bg_alpha() as f64;
        let k = if self.config.alpha_mode == wgpu::CompositeAlphaMode::PreMultiplied { a } else { 1.0 };
        let ch = |v: u8| {
            let c = v as f64 / 255.0 * k;
            if linear {
                srgb_to_linear(c)
            } else {
                c
            }
        };
        let clear = wgpu::Color { r: ch(bg[0]), g: ch(bg[1]), b: ch(bg[2]), a };
        let pipes = match (&self.linear_pipelines, linear) {
            (Some((_, p)), true) => p,
            _ => &self.raw_pipelines,
        };
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &tex_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.bind_group, &[]);
            if n_bg > 0 {
                pass.set_pipeline(&pipes.main);
                pass.set_vertex_buffer(0, self.inst_buf.slice(..));
                pass.draw(0..4, 0..n_bg as u32);
            }
            if n_poly > 0 {
                pass.set_pipeline(&pipes.poly);
                pass.set_vertex_buffer(0, self.poly_buf.slice(..));
                pass.draw(0..n_poly as u32, 0..1);
            }
            if n > n_bg {
                pass.set_pipeline(&pipes.main);
                pass.set_vertex_buffer(0, self.inst_buf.slice(..));
                pass.draw(0..4, n_bg as u32..n as u32);
            }
        }
        self.queue.submit(Some(enc.finish()));
        self.queue.present(frame);
    }
}
fn pick_alpha_mode(modes: &[wgpu::CompositeAlphaMode], opacity: f32) -> wgpu::CompositeAlphaMode {
    use wgpu::CompositeAlphaMode as M;
    let wanted: &[M] = if opacity < 1.0 { &[M::PreMultiplied, M::PostMultiplied] } else { &[M::Opaque] };
    if let Some(m) = wanted.iter().copied().find(|m| modes.contains(m)) {
        return m;
    }
    if opacity < 1.0 {
        log::error("aeternus: this GPU surface does not support transparency; window.opacity is ignored");
    }
    if modes.contains(&M::Opaque) {
        M::Opaque
    } else {
        modes.first().copied().unwrap_or(M::Auto)
    }
}
fn trail_config(cfg: &Config) -> TrailConfig {
    TrailConfig {
        still_ms: cfg.cursor.trail,
        fast: cfg.cursor.trail_decay[0],
        slow: cfg.cursor.trail_decay[1],
        threshold: cfg.cursor.trail_start_threshold as f32,
    }
}
fn measure_cell_width(fs: &mut FontSystem, family: &str, size: f32, line_h: f32) -> f32 {
    let mut buf = Buffer::new(fs, Metrics::new(size, line_h));
    let attrs = Attrs::new().family(Family::Name(family));
    buf.set_text("M", &attrs, Shaping::Advanced, None);
    buf.shape_until_scroll(fs, false);
    buf.layout_runs().next().and_then(|r| r.glyphs.first().map(|g| g.w)).unwrap_or(size * 0.6)
}

#[cfg(test)]
mod tests {
    use super::{is_powerline, is_private_use, is_symbol};

    #[test]
    fn nerd_font_ranges() {
        assert!(is_private_use('\u{e0b0}'));
        assert!(is_private_use('\u{f0001}'));
        assert!(!is_private_use('a'));
        assert!(is_symbol('\u{23fb}'));
        assert!(is_symbol('\u{2714}'));
        assert!(!is_symbol('─'));
        assert!(!is_symbol('é'));
        assert!(is_powerline('\u{e0b4}'));
        assert!(!is_powerline('\u{f07c}'));
    }
}
