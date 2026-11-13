use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use bytemuck::{Pod, Zeroable};
use cosmic_text::{
    Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Style, SwashCache, SwashContent, Weight,
};
use wgpu::util::DeviceExt;
use winit::event::KeyEvent;
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::Window;
use crate::cell::{flags, Cell};
use crate::colors::Palette;
use crate::config::Config;
use crate::term::{CursorShape, Term, WIDE_SPACER};
const ATLAS_SIZE: u32 = 2048;
const WHITE_UV: [f32; 2] = [1.0 / ATLAS_SIZE as f32, 1.0 / ATLAS_SIZE as f32];
const TOGGLE_ANIM: f32 = 0.12;
const SENTINEL: [u8; 3] = [1, 2, 3];
const ART_INK: [u8; 3] = [2, 4, 6];
pub struct View {
    pub focused: bool,
    pub blink_on: bool,
}
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    Copy,
    Paste,
    PastePrimary,
    SelectAll,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ScrollTop,
    ScrollBottom,
    ResetTerm,
    Reload,
    Settings,
    Quit,
    About,
    NewTab,
    CloseTab,
    Fullscreen,
}
pub struct MenuItem {
    pub label: &'static str,
    pub shortcut: Option<&'static str>,
    pub action: Option<Action>,
}
const SEP: MenuItem = MenuItem { label: "", shortcut: None, action: None };
const fn item(
    label: &'static str,
    shortcut: Option<&'static str>,
    action: Option<Action>,
) -> MenuItem {
    MenuItem { label, shortcut, action }
}
const MENUS: [(&str, &[MenuItem]); 4] = [
    (
        "Arquivo",
        &[
            item("Nova aba", Some("Ctrl+Shift+T"), Some(Action::NewTab)),
            item("Fechar aba", Some("Ctrl+Shift+W"), Some(Action::CloseTab)),
            SEP,
            item("Recarregar configuração", Some("Ctrl+Shift+R"), Some(Action::Reload)),
            SEP,
            item("Sair", None, Some(Action::Quit)),
        ],
    ),
    (
        "Editar",
        &[
            item("Copiar", Some("Ctrl+Shift+C"), Some(Action::Copy)),
            item("Colar", Some("Ctrl+Shift+V"), Some(Action::Paste)),
            item("Colar seleção", None, Some(Action::PastePrimary)),
            item("Selecionar tudo", None, Some(Action::SelectAll)),
            SEP,
            item("Preferências", None, Some(Action::Settings)),
        ],
    ),
    (
        "Ver",
        &[
            item("Rolar ao topo", Some("Shift+Home"), Some(Action::ScrollTop)),
            item("Rolar até o fim", Some("Shift+End"), Some(Action::ScrollBottom)),
            SEP,
            item("Ampliar", Some("Ctrl+Shift+="), Some(Action::ZoomIn)),
            item("Reduzir", Some("Ctrl+Shift+-"), Some(Action::ZoomOut)),
            item("Zoom padrão", Some("Ctrl+Shift+0"), Some(Action::ZoomReset)),
            SEP,
            item("Tela cheia", Some("F11"), Some(Action::Fullscreen)),
            SEP,
            item("Resetar terminal", None, Some(Action::ResetTerm)),
        ],
    ),
    ("Ajuda", &[item("Sobre o Aeternus", None, Some(Action::About))]),
];
struct PanelGeom {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    item_h: f32,
    sep_h: f32,
}
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SettingsOutcome {
    Saved,
    Cancelled,
}
#[derive(Clone, Copy, PartialEq)]
enum FieldKey {
    FontFamily,
    FontSize,
    LineHeight,
    WinWidth,
    WinHeight,
    Padding,
    Toolbar,
    Shell,
    WorkDir,
    Scrollback,
    Osc52,
    CursorShape,
    CursorBlink,
    Foreground,
    Background,
    Selection,
    CursorColor,
}
#[derive(Clone, Copy)]
enum FieldKind {
    Text,
    Number,
    Toggle,
    Color,
    Choice(&'static [&'static str]),
}
struct FieldSpec {
    key: FieldKey,
    label: &'static str,
    hint: &'static str,
    simple: bool,
    kind: FieldKind,
}
const FIELDS: [FieldSpec; 17] = [
    FieldSpec { key: FieldKey::FontFamily, label: "Família da fonte", hint: "Nome de uma fonte instalada no sistema.", simple: true, kind: FieldKind::Text },
    FieldSpec { key: FieldKey::FontSize, label: "Tamanho da fonte", hint: "Pixels lógicos (4 a 128).", simple: true, kind: FieldKind::Number },
    FieldSpec { key: FieldKey::LineHeight, label: "Espaçamento entre linhas", hint: "Multiplicador do tamanho da fonte (1.0 a 3.0).", simple: false, kind: FieldKind::Number },
    FieldSpec { key: FieldKey::WinWidth, label: "Largura da janela", hint: "Em pixels; só vale ao reabrir.", simple: false, kind: FieldKind::Number },
    FieldSpec { key: FieldKey::WinHeight, label: "Altura da janela", hint: "Em pixels; só vale ao reabrir.", simple: false, kind: FieldKind::Number },
    FieldSpec { key: FieldKey::Padding, label: "Margem interna", hint: "Espaço entre a borda e o texto (0 a 200).", simple: false, kind: FieldKind::Number },
    FieldSpec { key: FieldKey::Toolbar, label: "Barra de menus", hint: "Mostra os menus no topo da janela.", simple: true, kind: FieldKind::Toggle },
    FieldSpec { key: FieldKey::Shell, label: "Shell", hint: "Vazio usa $SHELL; aceita argumentos, ex.: bash -l; só vale ao reabrir.", simple: false, kind: FieldKind::Text },
    FieldSpec { key: FieldKey::WorkDir, label: "Diretório inicial", hint: "Vazio = diretório atual; só vale ao reabrir.", simple: false, kind: FieldKind::Text },
    FieldSpec { key: FieldKey::Scrollback, label: "Linhas de histórico", hint: "Scrollback guardado (até 1.000.000).", simple: false, kind: FieldKind::Number },
    FieldSpec { key: FieldKey::Osc52, label: "OSC 52 (clipboard)", hint: "Programas podem copiar via OSC 52.", simple: false, kind: FieldKind::Toggle },
    FieldSpec { key: FieldKey::CursorShape, label: "Forma do cursor", hint: "Clique para alternar: block, underline, bar.", simple: false, kind: FieldKind::Choice(&["block", "underline", "bar"]) },
    FieldSpec { key: FieldKey::CursorBlink, label: "Piscar cursor", hint: "Cursor piscando quando ocioso.", simple: false, kind: FieldKind::Toggle },
    FieldSpec { key: FieldKey::Foreground, label: "Cor do texto", hint: "Hex #rrggbb.", simple: true, kind: FieldKind::Color },
    FieldSpec { key: FieldKey::Background, label: "Cor do fundo", hint: "Hex #rrggbb.", simple: true, kind: FieldKind::Color },
    FieldSpec { key: FieldKey::Selection, label: "Cor da seleção", hint: "Hex #rrggbb.", simple: true, kind: FieldKind::Color },
    FieldSpec { key: FieldKey::CursorColor, label: "Cor do cursor", hint: "Vazio = mesma cor do texto. Hex #rrggbb.", simple: true, kind: FieldKind::Color },
];
const SECTIONS: &[(&str, &[FieldKey])] = &[
    ("Fonte", &[FieldKey::FontFamily, FieldKey::FontSize, FieldKey::LineHeight]),
    ("Janela", &[FieldKey::WinWidth, FieldKey::WinHeight, FieldKey::Padding, FieldKey::Toolbar]),
    ("Terminal", &[FieldKey::Shell, FieldKey::WorkDir, FieldKey::Scrollback, FieldKey::Osc52]),
    ("Cursor", &[FieldKey::CursorShape, FieldKey::CursorBlink]),
    ("Cores", &[FieldKey::Foreground, FieldKey::Background, FieldKey::Selection, FieldKey::CursorColor]),
];
enum FieldValue {
    Text(String),
    Toggle(bool),
    Choice(usize),
}
struct SettingsState {
    scroll: f32,
    focus: Option<usize>,
    draft: Vec<FieldValue>,
    simple: bool,
    picker: Option<PickerState>,
    anims: Vec<(usize, Instant, f32)>,
}
#[derive(Clone, Copy)]
struct PickerState {
    field: usize,
    h: f32,
    s: f32,
    v: f32,
    drag: u8,
}
struct SRow {
    sec: usize,
    fld: Option<usize>,
    y: f32,
    h: f32,
}
struct SettingsGeom {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    rows_top: f32,
    rows_bottom: f32,
}
struct PickerGeom {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    sv: (f32, f32, f32, f32),
    hue: (f32, f32, f32, f32),
    preview: (f32, f32, f32, f32),
    ok: (f32, f32, f32, f32),
    cancel: (f32, f32, f32, f32),
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
        Self {
            pos: [x, y],
            size: [w, h],
            uv_pos: WHITE_UV,
            uv_size: [0.0, 0.0],
            color: color4(rgb),
            color_glyph: 0.0,
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
fn square_outline(out: &mut Vec<Instance>, x: f32, y: f32, s: f32, thick: f32, col: [u8; 3]) {
    out.push(Instance::rect(x, y, s, thick, col));
    out.push(Instance::rect(x, y + s - thick, s, thick, col));
    out.push(Instance::rect(x, y, thick, s, col));
    out.push(Instance::rect(x + s - thick, y, thick, s, col));
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
    let i = hh.floor() as i32 % 6;
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
fn settings_rows(simple: bool) -> Vec<(usize, Option<usize>)> {
    let mut rows = Vec::new();
    for (si, (_, keys)) in SECTIONS.iter().enumerate() {
        let mut flds = Vec::new();
        for k in *keys {
            if let Some(fi) = FIELDS.iter().position(|f| f.key == *k) {
                if !simple || FIELDS[fi].simple {
                    flds.push(Some(fi));
                }
            }
        }
        if !flds.is_empty() {
            rows.push((si, None));
            rows.extend(flds.into_iter().map(|f| (si, f)));
        }
    }
    rows
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
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }
}
pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    globals: wgpu::Buffer,
    inst_buf: wgpu::Buffer,
    inst_cap: usize,
    atlas: Atlas,
    glyphs: HashMap<(char, bool, bool), Option<GlyphInfo>>,
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
    effective_bg: [u8; 3],
    atlas_reset: bool,
    toolbar: bool,
    toolbar_h: f32,
    title_rects: Vec<(f32, f32)>,
    open_menu: Option<usize>,
    hover_title: Option<usize>,
    hover_item: Option<usize>,
    settings: Option<SettingsState>,
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
}
impl Renderer {
    pub fn new(window: Arc<Window>, cfg: &Config) -> Self {
        let size = window.inner_size();
        let scale = window.scale_factor() as f32;
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window).expect("surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("nenhum adaptador de GPU");
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None))
                .expect("device");
        let caps = surface.get_capabilities(&adapter);
        let format = caps.formats.iter().copied().find(|f| !f.is_srgb()).unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        let atlas = Atlas::new(&device, &queue);
        let view = atlas.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let globals = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("globals"),
            contents: bytemuck::cast_slice(&[config.width as f32, config.height as f32, 0.0, 0.0]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
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
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pl_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                compilation_options: Default::default(),
                buffers: &[Instance::layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let inst_cap = 16384;
        let inst_buf = Self::make_inst_buf(&device, inst_cap);
        let mut font_system = FontSystem::new();
        let font_size = cfg.font.size * scale;
        let cell_h = (font_size * cfg.font.line_height).round().max(1.0);
        let cell_w = measure_cell_width(&mut font_system, &cfg.font.family, font_size, cell_h)
            .round()
            .max(1.0);
        Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            bind_group,
            globals,
            inst_buf,
            inst_cap,
            atlas,
            glyphs: HashMap::new(),
            ascii: vec![None; 128 * 4],
            font_system,
            swash: SwashCache::new(),
            font_family: cfg.font.family.clone(),
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
            effective_bg: cfg.palette.bg,
            atlas_reset: false,
            toolbar: cfg.window.toolbar,
            toolbar_h: if cfg.window.toolbar {
                (36.0 * scale).round()
            } else {
                0.0
            },
            title_rects: Vec::new(),
            open_menu: None,
            hover_title: None,
            hover_item: None,
            settings: None,
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
    pub fn pad(&self) -> f32 {
        self.pad
    }
    pub fn top_inset(&self) -> f32 {
        self.toolbar_h + self.tab_h()
    }
    fn tab_h(&self) -> f32 {
        if self.tabs.len() > 1 {
            (28.0 * self.scale).round()
        } else {
            0.0
        }
    }
    pub fn tabs_visible(&self) -> bool {
        self.tabs.len() > 1
    }
    pub fn toolbar_h(&self) -> f32 {
        self.toolbar_h
    }
    pub fn set_tabs(&mut self, tabs: &[(u64, String)], active: u64) -> bool {
        let changed = self.tabs != *tabs || self.active_tab != active;
        self.tabs = tabs.to_vec();
        self.active_tab = active;
        changed
    }
    pub fn set_hover_tab(&mut self, t: Option<u64>) -> bool {
        if self.hover_tab == t { return false; }
        self.hover_tab = t;
        true
    }
    pub fn set_hover_close(&mut self, t: Option<u64>) -> bool {
        if self.hover_close == t { return false; }
        self.hover_close = t;
        true
    }
    pub fn set_hover_new(&mut self, h: bool) -> bool {
        if self.hover_new == h { return false; }
        self.hover_new = h;
        true
    }
    pub fn set_hover_gear(&mut self, h: bool) -> bool {
        if self.hover_gear == h { return false; }
        self.hover_gear = h;
        true
    }
    pub fn clear_hover(&mut self) -> bool {
        let mut c = self.set_hover_tab(None);
        c |= self.set_hover_close(None);
        c |= self.set_hover_new(false);
        c |= self.set_hover_prev(false);
        c |= self.set_hover_next(false);
        c
    }
    pub fn hit_tab(&self, x: f64, y: f64) -> Option<u64> {
        if self.tabs.len() < 2 { return None; }
        let y = y as f32;
        if y < self.toolbar_h || y >= self.toolbar_h + self.tab_h() { return None; }
        let x = x as f32;
        self.tab_rects.iter().find(|&&(tx, tw, _)| x >= tx && x < tx + tw).map(|&(_, _, id)| id)
    }
    pub fn hit_tab_close(&self, x: f64, y: f64) -> Option<u64> {
        if self.tabs.len() < 2 { return None; }
        let y = y as f32;
        if y < self.toolbar_h || y >= self.toolbar_h + self.tab_h() { return None; }
        let x = x as f32;
        self.close_rects.iter().find(|&&(cx, cw, _)| x >= cx && x < cx + cw).map(|&(_, _, id)| id)
    }
    pub fn hit_new_tab(&self, x: f64, y: f64) -> bool {
        if self.tabs.len() < 2 { return false; }
        let y = y as f32;
        if y < self.toolbar_h || y >= self.toolbar_h + self.tab_h() { return false; }
        let Some((nx, nw)) = self.new_tab_rect else { return false };
        let x = x as f32;
        x >= nx && x < nx + nw
    }
    pub fn hit_gear(&self, x: f64, y: f64) -> bool {
        let Some((gx, gw)) = self.gear_rect else { return false };
        let y = y as f32;
        if y < 0.0 || y >= self.toolbar_h { return false; }
        let x = x as f32;
        x >= gx && x < gx + gw
    }
    fn tab_widths(&self) -> Vec<f32> {
        let cw = self.cell_w;
        self.tabs
            .iter()
            .map(|(_, label)| {
                let n = label.chars().count();
                let wch = if n > 18 { 18 } else { n };
                (wch as f32 + 4.0) * cw
            })
            .collect()
    }
    pub fn tab_strip_hit(&self, _x: f64, y: f64) -> bool {
        if self.tabs.len() < 2 {
            return false;
        }
        let y = y as f32;
        y >= self.toolbar_h && y < self.toolbar_h + self.tab_h()
    }
    pub fn tab_strip_scroll(&mut self, delta: isize) -> bool {
        if self.tabs.len() < 2 {
            return false;
        }
        let max = (self.tabs.len() - 1) as isize;
        let old = self.tab_scroll;
        self.tab_scroll = (self.tab_scroll as isize + delta).clamp(0, max) as usize;
        self.tab_scroll != old
    }
    pub fn ensure_tab_visible(&mut self, id: u64) {
        if self.tabs.len() < 2 {
            return;
        }
        let Some(idx) = self.tabs.iter().position(|t| t.0 == id) else {
            return;
        };
        let widths = self.tab_widths();
        let cw = self.cell_w;
        let w = self.config.width as f32;
        let nw = 2.0 * cw;
        let arrow_w = 2.0 * cw;
        let limit = w - self.pad - nw - cw * 0.25 - 2.0 * arrow_w - cw * 0.5;
        let mut scroll = self.tab_scroll.min(idx);
        loop {
            let mut x = self.pad;
            let mut k = 0usize;
            while scroll + k < widths.len() {
                if x + widths[scroll + k] > limit {
                    break;
                }
                x += widths[scroll + k];
                k += 1;
            }
            if idx < scroll + k || scroll >= idx {
                break;
            }
            scroll += 1;
        }
        self.tab_scroll = scroll;
    }
    fn tab_arrow_hit(&self, r: Option<(f32, f32)>, x: f64, y: f64) -> bool {
        let Some((ax, aw)) = r else {
            return false;
        };
        let y = y as f32;
        if y < self.toolbar_h || y >= self.toolbar_h + self.tab_h() {
            return false;
        }
        let x = x as f32;
        x >= ax && x < ax + aw
    }
    pub fn hit_tab_prev(&self, x: f64, y: f64) -> bool {
        self.tab_arrow_hit(self.tab_prev_rect, x, y)
    }
    pub fn hit_tab_next(&self, x: f64, y: f64) -> bool {
        self.tab_arrow_hit(self.tab_next_rect, x, y)
    }
    pub fn set_hover_prev(&mut self, h: bool) -> bool {
        if self.hover_prev == h {
            return false;
        }
        self.hover_prev = h;
        true
    }
    pub fn set_hover_next(&mut self, h: bool) -> bool {
        if self.hover_next == h {
            return false;
        }
        self.hover_next = h;
        true
    }
    pub fn menu_open(&self) -> Option<usize> {
        self.open_menu
    }
    pub fn set_open_menu(&mut self, m: Option<usize>) -> bool {
        if self.open_menu == m {
            return false;
        }
        self.open_menu = m;
        true
    }
    pub fn set_hover_title(&mut self, t: Option<usize>) -> bool {
        if self.hover_title == t {
            return false;
        }
        self.hover_title = t;
        true
    }
    pub fn set_hover_item(&mut self, i: Option<usize>) -> bool {
        if self.hover_item == i {
            return false;
        }
        self.hover_item = i;
        true
    }
    pub fn open_settings(&mut self, cfg: &Config) {
        let draft = FIELDS
            .iter()
            .map(|f| match f.key {
                FieldKey::FontFamily => FieldValue::Text(cfg.font.family.clone()),
                FieldKey::FontSize => FieldValue::Text(cfg.font.size.to_string()),
                FieldKey::LineHeight => FieldValue::Text(cfg.font.line_height.to_string()),
                FieldKey::WinWidth => FieldValue::Text(cfg.window.width.to_string()),
                FieldKey::WinHeight => FieldValue::Text(cfg.window.height.to_string()),
                FieldKey::Padding => FieldValue::Text(cfg.window.padding.to_string()),
                FieldKey::Toolbar => FieldValue::Toggle(cfg.window.toolbar),
                FieldKey::Shell => FieldValue::Text(cfg.terminal.shell.clone()),
                FieldKey::WorkDir => FieldValue::Text(cfg.terminal.working_directory.clone()),
                FieldKey::Scrollback => FieldValue::Text(cfg.terminal.scrollback.to_string()),
                FieldKey::Osc52 => FieldValue::Toggle(cfg.terminal.osc52),
                FieldKey::CursorShape => FieldValue::Choice(match cfg.cursor.shape {
                    CursorShape::Underline => 1,
                    CursorShape::Bar => 2,
                    _ => 0,
                }),
                FieldKey::CursorBlink => FieldValue::Toggle(cfg.cursor.blink),
                FieldKey::Foreground => FieldValue::Text(cfg.colors.foreground.clone()),
                FieldKey::Background => FieldValue::Text(cfg.colors.background.clone()),
                FieldKey::Selection => FieldValue::Text(cfg.colors.selection.clone()),
                FieldKey::CursorColor => FieldValue::Text(cfg.colors.cursor.clone()),
            })
            .collect();
        self.settings = Some(SettingsState {
            scroll: 0.0,
            focus: None,
            draft,
            simple: true,
            picker: None,
            anims: Vec::new(),
        });
    }
    pub fn close_settings(&mut self) {
        self.settings = None;
    }
    pub fn settings_open(&self) -> bool {
        self.settings.is_some()
    }
    pub fn picker_release(&mut self) -> bool {
        let Some(s) = self.settings.as_mut() else { return false };
        let Some(pk) = s.picker.as_mut() else { return false };
        if pk.drag == 0 {
            return false;
        }
        pk.drag = 0;
        true
    }
    pub fn settings_anim_wake(&mut self, now: Instant) -> Option<Instant> {
        let s = self.settings.as_mut()?;
        let mut next: Option<Instant> = None;
        s.anims.retain(|&(_, t0, _)| {
            let end = t0 + Duration::from_secs_f32(TOGGLE_ANIM);
            if now >= end {
                false
            } else {
                next = Some(match next {
                    Some(n) => n.min(end),
                    None => end,
                });
                true
            }
        });
        next
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
    pub fn settings_scroll(&mut self, delta: f32) -> bool {
        if self.settings.is_none() {
            return false;
        }
        if self.settings.as_ref().map_or(false, |s| s.picker.is_some()) {
            return true;
        }
        let simple = self.settings.as_ref().map_or(true, |s| s.simple);
        let Some(g) = self.settings_geom() else { return true };
        let content = self.settings_layout(&g, simple, 0.0).1;
        let cell_h = self.cell_h;
        let Some(s) = &mut self.settings else { return false };
        s.scroll = (s.scroll + delta * cell_h)
            .clamp(0.0, (content - (g.rows_bottom - g.rows_top)).max(0.0));
        true
    }
    pub fn settings_click(&mut self, x: f64, y: f64) -> Option<SettingsOutcome> {
        let g = self.settings_geom()?;
        let (cw, ch) = (self.cell_w, self.cell_h);
        let (x, y) = (x as f32, y as f32);
        if self.settings.as_ref().map_or(false, |s| s.picker.is_some()) {
            self.picker_click(x, y);
            return None;
        }
        let inside_panel = x >= g.x && x < g.x + g.w && y >= g.y && y < g.y + g.h;
        if !inside_panel {
            return Some(SettingsOutcome::Cancelled);
        }
        let simple = self.settings.as_ref().map_or(true, |s| s.simple);
        let scroll = self.settings.as_ref().map_or(0.0, |s| s.scroll);
        let rows = self.settings_layout(&g, simple, scroll).0;
        let (sx, sy, sw, _) = self.settings_seg(&g);
        let s = self.settings.as_mut()?;
        let bw = 10.0 * cw;
        let bh = ch * 1.8;
        let by = g.y + g.h - ch * 0.7 - bh;
        let save_x = g.x + g.w - cw * 1.5 - bw;
        let cancel_x = save_x - cw * 0.75 - bw;
        if y >= by && y < by + bh {
            if x >= save_x && x < save_x + bw {
                return Some(SettingsOutcome::Saved);
            }
            if x >= cancel_x && x < cancel_x + bw {
                return Some(SettingsOutcome::Cancelled);
            }
        }
        if y >= sy && y < sy + ch * 1.7 && x >= sx && x < sx + 2.0 * sw {
            let simple_now = x < sx + sw;
            if simple_now != s.simple {
                s.simple = simple_now;
                s.scroll = 0.0;
                s.focus = None;
            }
            return None;
        }
        if y < g.rows_top || y >= g.rows_bottom {
            return None;
        }
        let Some(row) = rows.iter().find(|r| y >= r.y && y < r.y + r.h) else {
            return None;
        };
        let Some(fi) = row.fld else {
            return None;
        };
        s.focus = Some(fi);
        match FIELDS[fi].kind {
            FieldKind::Toggle => {
                if let FieldValue::Toggle(v) = &mut s.draft[fi] {
                    let from = if *v { 1.0 } else { 0.0 };
                    *v = !*v;
                    s.anims.retain(|a| a.0 != fi);
                    s.anims.push((fi, Instant::now(), from));
                }
            }
            FieldKind::Choice(opts) => {
                if let FieldValue::Choice(i) = &mut s.draft[fi] {
                    *i = (*i + 1) % opts.len();
                }
            }
            FieldKind::Color => {
                let cur = match &s.draft[fi] {
                    FieldValue::Text(t) => crate::colors::parse_hex(t).unwrap_or([255, 255, 255]),
                    _ => [255, 255, 255],
                };
                let (h, sa, v) = rgb_to_hsv(cur);
                s.picker = Some(PickerState { field: fi, h, s: sa, v, drag: 0 });
            }
            _ => {}
        }
        None
    }
    pub fn settings_key(&mut self, ev: &KeyEvent, mods: ModifiersState) -> bool {
        let Some(s) = &mut self.settings else { return false };
        let picker_field = s.picker.as_ref().map(|p| (p.field, p.h, p.s, p.v));
        if let Some((f, h, sa, v)) = picker_field {
            match &ev.logical_key {
                Key::Named(NamedKey::Escape) => s.picker = None,
                Key::Named(NamedKey::Enter) => {
                    if let FieldValue::Text(t) = &mut s.draft[f] {
                        *t = hex_str(hsv_to_rgb(h, sa, v));
                    }
                    s.picker = None;
                }
                _ => {}
            }
            return true;
        }
        let Some(idx) = s.focus else { return true };
        if !matches!(
            FIELDS[idx].kind,
            FieldKind::Text | FieldKind::Number | FieldKind::Color
        ) {
            return true;
        }
        let FieldValue::Text(text) = &mut s.draft[idx] else { return true };
        match &ev.logical_key {
            Key::Named(NamedKey::Backspace) => {
                text.pop();
            }
            Key::Named(NamedKey::Space) => text.push(' '),
            Key::Named(_) => {}
            Key::Character(c) => {
                if !mods.control_key() && !mods.alt_key() && !mods.super_key() {
                    text.push_str(c.as_str());
                }
            }
            Key::Dead(_) | Key::Unidentified(_) => {}
        }
        true
    }
    pub fn settings_writeback(&self, cfg: &mut Config) {
        let Some(s) = &self.settings else { return };
        for (i, spec) in FIELDS.iter().enumerate() {
            match &s.draft[i] {
                FieldValue::Toggle(v) => match spec.key {
                    FieldKey::Toolbar => cfg.window.toolbar = *v,
                    FieldKey::Osc52 => cfg.terminal.osc52 = *v,
                    FieldKey::CursorBlink => cfg.cursor.blink = *v,
                    _ => {}
                },
                FieldValue::Choice(idx) => {
                    let FieldKind::Choice(opts) = spec.kind else { unreachable!() };
                    if spec.key == FieldKey::CursorShape {
                        cfg.cursor.shape = match opts[*idx] {
                            "underline" => CursorShape::Underline,
                            "bar" => CursorShape::Bar,
                            _ => CursorShape::Block,
                        };
                    }
                }
                FieldValue::Text(text) => {
                    let text = text.trim();
                    match spec.key {
                        FieldKey::FontFamily => cfg.font.family = text.to_string(),
                        FieldKey::FontSize => {
                            if let Ok(v) = text.parse::<f32>() {
                                cfg.font.size = v;
                            }
                        }
                        FieldKey::LineHeight => {
                            if let Ok(v) = text.parse::<f32>() {
                                cfg.font.line_height = v;
                            }
                        }
                        FieldKey::WinWidth => {
                            if let Ok(v) = text.parse::<u32>() {
                                cfg.window.width = v;
                            }
                        }
                        FieldKey::WinHeight => {
                            if let Ok(v) = text.parse::<u32>() {
                                cfg.window.height = v;
                            }
                        }
                        FieldKey::Padding => {
                            if let Ok(v) = text.parse::<f32>() {
                                cfg.window.padding = v;
                            }
                        }
                        FieldKey::Shell => cfg.terminal.shell = text.to_string(),
                        FieldKey::WorkDir => cfg.terminal.working_directory = text.to_string(),
                        FieldKey::Scrollback => {
                            if let Ok(v) = text.parse::<usize>() {
                                cfg.terminal.scrollback = v;
                            }
                        }
                        FieldKey::Foreground => cfg.colors.foreground = text.to_string(),
                        FieldKey::Background => cfg.colors.background = text.to_string(),
                        FieldKey::Selection => cfg.colors.selection = text.to_string(),
                        FieldKey::CursorColor => cfg.colors.cursor = text.to_string(),
                        _ => {}
                    }
                }
            }
        }
    }
    fn settings_geom(&self) -> Option<SettingsGeom> {
        self.settings.as_ref()?;
        let (cw, ch) = (self.cell_w, self.cell_h);
        let pad = self.pad + cw;
        let x = pad;
        let y = self.top_inset() + self.pad;
        let w = self.config.width as f32 - 2.0 * pad;
        let h = self.config.height as f32 - y - self.pad;
        if w <= 0.0 || h <= 0.0 {
            return None;
        }
        Some(SettingsGeom {
            x,
            y,
            w,
            h,
            rows_top: y + ch * 3.0,
            rows_bottom: y + h - ch * 2.8,
        })
    }
    fn settings_layout(&self, g: &SettingsGeom, simple: bool, scroll: f32) -> (Vec<SRow>, f32) {
        let ch = self.cell_h;
        let sec_h = (ch * 2.4).round();
        let row_h = (ch * 2.6).round();
        let gap = (ch * 0.8).round();
        let mut rows = Vec::new();
        let mut y = g.rows_top;
        let mut prev_fld = false;
        for (si, fld) in settings_rows(simple) {
            if fld.is_none() && prev_fld {
                y += gap;
            }
            let h = if fld.is_none() { sec_h } else { row_h };
            rows.push(SRow { sec: si, fld, y: y - scroll, h });
            y += h;
            prev_fld = fld.is_some();
        }
        (rows, y + gap - g.rows_top)
    }
    fn settings_seg(&self, g: &SettingsGeom) -> (f32, f32, f32, f32) {
        let (cw, ch) = (self.cell_w, self.cell_h);
        let w = 8.5 * cw;
        (g.x + g.w - cw * 1.5 - 2.0 * w, g.y + ch * 0.35, w, ch * 1.7)
    }
    fn picker_geom(&self) -> Option<PickerGeom> {
        self.settings.as_ref()?.picker.as_ref()?;
        let (cw, ch) = (self.cell_w, self.cell_h);
        let w = self.config.width as f32;
        let h = self.config.height as f32;
        let pw = (30.0 * cw).min(w - 2.0 * self.pad).max(20.0 * cw);
        let ph = (ch * 15.5).min(h - 2.0 * self.pad);
        if ph < ch * 10.0 {
            return None;
        }
        let x = ((w - pw) / 2.0).floor();
        let y = ((h - ph) / 2.0).floor();
        let inner_x = x + cw;
        let inner_w = pw - 2.0 * cw;
        Some(PickerGeom {
            x,
            y,
            w: pw,
            h: ph,
            sv: (inner_x, y + ch * 1.9, inner_w, ch * 6.5),
            hue: (inner_x, y + ch * 9.0, inner_w, ch * 1.5),
            preview: (inner_x, y + ch * 11.2, inner_w, ch * 1.9),
            ok: (x + pw - cw - 10.0 * cw, y + ph - ch * 2.2, 10.0 * cw, ch * 1.7),
            cancel: (x + pw - cw - 21.0 * cw, y + ph - ch * 2.2, 10.0 * cw, ch * 1.7),
        })
    }
    fn picker_click(&mut self, x: f32, y: f32) {
        let Some(g) = self.picker_geom() else { return };
        let Some(pk) = self.settings.as_mut().and_then(|s| s.picker.as_mut()) else { return };
        let (sx, sy, sw, sh) = g.sv;
        if x >= sx && x < sx + sw && y >= sy && y < sy + sh {
            pk.s = ((x - sx) / sw).clamp(0.0, 1.0);
            pk.v = 1.0 - ((y - sy) / sh).clamp(0.0, 1.0);
            pk.drag = 1;
            return;
        }
        let (hx, hy, hw, hh) = g.hue;
        if x >= hx && x < hx + hw && y >= hy && y < hy + hh {
            pk.h = ((x - hx) / hw * 360.0).clamp(0.0, 359.99);
            pk.drag = 2;
            return;
        }
        let (ox, oy, ow, oh) = g.ok;
        if x >= ox && x < ox + ow && y >= oy && y < oy + oh {
            let (h, s, v) = (pk.h, pk.s, pk.v);
            let f = pk.field;
            if let Some(s2) = self.settings.as_mut() {
                if let FieldValue::Text(t) = &mut s2.draft[f] {
                    *t = hex_str(hsv_to_rgb(h, s, v));
                }
                s2.picker = None;
            }
            return;
        }
        let (cx, cy, cw2, ch2) = g.cancel;
        if x >= cx && x < cx + cw2 && y >= cy && y < cy + ch2 {
            if let Some(s2) = self.settings.as_mut() {
                s2.picker = None;
            }
            return;
        }
        if x < g.x || x >= g.x + g.w || y < g.y || y >= g.y + g.h {
            if let Some(s2) = self.settings.as_mut() {
                s2.picker = None;
            }
        }
    }
    pub fn settings_move(&mut self, x: f64, y: f64) -> bool {
        let Some(g) = self.picker_geom() else { return false };
        let Some(pk) = self.settings.as_mut().and_then(|s| s.picker.as_mut()) else { return false };
        match pk.drag {
            1 => {
                let (sx, sy, sw, sh) = g.sv;
                pk.s = ((x as f32 - sx) / sw).clamp(0.0, 1.0);
                pk.v = 1.0 - ((y as f32 - sy) / sh).clamp(0.0, 1.0);
                true
            }
            2 => {
                let (hx, _, hw, _) = g.hue;
                pk.h = ((x as f32 - hx) / hw * 360.0).clamp(0.0, 359.99);
                true
            }
            _ => false,
        }
    }
    pub fn hit_title(&self, x: f64, y: f64) -> Option<usize> {
        if !self.toolbar || y as f32 > self.toolbar_h {
            return None;
        }
        let x = x as f32;
        self.title_rects.iter().position(|&(tx, tw)| x >= tx && x < tx + tw)
    }
    pub fn hit_item_index(&self, x: f64, y: f64) -> Option<usize> {
        let m = self.open_menu?;
        let g = self.panel_geom(m)?;
        let (x, y) = (x as f32, y as f32);
        if x < g.x || x >= g.x + g.w || y < g.y {
            return None;
        }
        let mut ry = g.y + self.cell_h / 2.0;
        for (i, it) in MENUS[m].1.iter().enumerate() {
            let rh = if it.action.is_none() { g.sep_h } else { g.item_h };
            if y >= ry && y < ry + rh {
                return if it.action.is_none() { None } else { Some(i) };
            }
            ry += rh;
        }
        None
    }
    pub fn item_action(&self, m: usize, i: usize) -> Option<Action> {
        MENUS.get(m)?.1.get(i)?.action
    }
    pub fn panel_contains(&self, x: f64, y: f64) -> bool {
        let Some(m) = self.open_menu else { return false };
        let Some(g) = self.panel_geom(m) else { return false };
        let (x, y) = (x as f32, y as f32);
        x >= g.x && x < g.x + g.w && y >= g.y && y < g.y + g.h
    }
    fn panel_geom(&self, m: usize) -> Option<PanelGeom> {
        let &(tx, _) = self.title_rects.get(m)?;
        let (cw, ch) = (self.cell_w, self.cell_h);
        let item_h = (ch * 1.6).round();
        let sep_h = (item_h / 3.0).round().max(2.0);
        let mut w: f32 = 0.0;
        for it in MENUS[m].1 {
            if it.action.is_none() {
                continue;
            }
            let mut need = it.label.chars().count() as f32 * cw + 2.0 * cw;
            if let Some(s) = it.shortcut {
                need += s.chars().count() as f32 * cw + 2.0 * cw;
            }
            w = w.max(need);
        }
        w += 2.0 * cw;
        let mut h = 0.0;
        for it in MENUS[m].1 {
            h += if it.action.is_none() { sep_h } else { item_h };
        }
        h += ch;
        Some(PanelGeom { x: tx, y: self.toolbar_h, w, h, item_h, sep_h })
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
        self.cell_w = measure_cell_width(
            &mut self.font_system,
            &self.font_family,
            self.font_size,
            self.cell_h,
        )
        .round()
        .max(1.0);
        self.reset_atlas();
    }
    pub fn set_zoom(&mut self, z: f32) {
        self.zoom = (z.clamp(0.5, 4.0) * 20.0).round() / 20.0;
        self.recompute_metrics();
    }
    pub fn apply_config(&mut self, cfg: &Config) {
        self.font_family = cfg.font.family.clone();
        self.base_font_size = cfg.font.size;
        self.line_height = cfg.font.line_height;
        self.window_padding = cfg.window.padding;
        self.pad = (self.window_padding * self.scale).round();
        self.toolbar = cfg.window.toolbar;
        self.toolbar_h = if cfg.window.toolbar {
            (36.0 * self.scale).round()
        } else {
            0.0
        };
        self.palette = cfg.palette.clone();
        self.recompute_metrics();
    }

    pub fn set_scale(&mut self, scale: f32) {
        if scale == self.scale {
            return;
        }
        self.scale = scale;
        self.pad = (self.window_padding * self.scale).round();
        self.recompute_metrics();
    }
    pub fn grid_size(&self) -> (usize, usize) {
        let w = self.config.width as f32 - 2.0 * self.pad;
        let h = self.config.height as f32 - 2.0 * self.pad - self.top_inset();
        let cols = (w / self.cell_w) as usize;
        let rows = (h / self.cell_h) as usize;
        (cols.max(1), rows.max(1))
    }
    pub fn cell_at(&self, x: f64, y: f64) -> (usize, usize) {
        let (cols, rows) = self.grid_size();
        let c = ((x as f32 - self.pad) / self.cell_w).floor().max(0.0) as usize;
        let r = ((y as f32 - self.pad - self.top_inset()) / self.cell_h).floor().max(0.0) as usize;
        (r.min(rows - 1), c.min(cols - 1))
    }
    pub fn resize(&mut self, w: u32, h: u32) {
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
        self.queue.write_buffer(
            &self.globals,
            0,
            bytemuck::cast_slice(&[w as f32, h as f32, 0.0, 0.0]),
        );
    }
    fn glyph(&mut self, ch: char, bold: bool, italic: bool) -> Option<GlyphInfo> {
        if (ch as u32) < 128 {
            let idx = ((ch as usize) << 2) | ((bold as usize) << 1) | italic as usize;
            if let Some(g) = self.ascii[idx] {
                return g;
            }
            let g = self.rasterize(ch, bold, italic);
            self.ascii[idx] = Some(g);
            return g;
        }
        let key = (ch, bold, italic);
        if let Some(g) = self.glyphs.get(&key) {
            return *g;
        }
        let g = self.rasterize(ch, bold, italic);
        self.glyphs.insert(key, g);
        g
    }
    fn reset_atlas(&mut self) {
        self.atlas.reset();
        self.glyphs.clear();
        self.ascii.fill(None);
        self.atlas_reset = true;
    }
    fn rasterize(&mut self, ch: char, bold: bool, italic: bool) -> Option<GlyphInfo> {
        let family = self.font_family.clone();
        self.rasterize_in(ch, &family, self.font_size, self.cell_h, bold, italic)
    }
    fn rasterize_in(
        &mut self,
        ch: char,
        family: &str,
        font_size: f32,
        line_h: f32,
        bold: bool,
        italic: bool,
    ) -> Option<GlyphInfo> {
        let attrs = Attrs::new()
            .family(Family::Name(family))
            .weight(if bold { Weight::BOLD } else { Weight::NORMAL })
            .style(if italic { Style::Italic } else { Style::Normal });
        let mut buf = Buffer::new(&mut self.font_system, Metrics::new(font_size, line_h));
        let mut tmp = [0u8; 4];
        buf.set_text(&mut self.font_system, ch.encode_utf8(&mut tmp), attrs, Shaping::Advanced);
        let run = buf.layout_runs().next()?;
        let line_y = run.line_y;
        let phys = run.glyphs.first()?.physical((0.0, 0.0), 1.0);
        let image = self.swash.get_image(&mut self.font_system, phys.cache_key).as_ref()?;
        let p = image.placement;
        if p.width == 0 || p.height == 0 {
            return None;
        }
        let (is_color, rgba): (bool, Vec<u8>) = match image.content {
            SwashContent::Mask => {
                (false, image.data.iter().flat_map(|&a| [a, 0, 0, 0]).collect())
            }
            SwashContent::Color => (true, image.data.clone()),
            _ => return None,
        };
        let (ax, ay) = match self.atlas.alloc(p.width, p.height) {
            Some(pos) => pos,
            None => {
                self.reset_atlas();
                self.atlas.alloc(p.width, p.height)?
            }
        };
        self.atlas.write(&self.queue, ax, ay, p.width, p.height, &rgba);
        let a = ATLAS_SIZE as f32;
        Some(GlyphInfo {
            uv_pos: [ax as f32 / a, ay as f32 / a],
            uv_size: [p.width as f32 / a, p.height as f32 / a],
            off: [(phys.x + p.left) as f32, (line_y as i32 + phys.y - p.top) as f32],
            size: [p.width as f32, p.height as f32],
            is_color,
        })
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
        let (cw, ch, pad) = (self.cell_w, self.cell_h, self.pad);
        let thick = self.scale.round().max(1.0);
        let cols = term.cols();
        let mut title_rects = std::mem::take(&mut self.title_rects);
        title_rects.clear();
        let w = self.config.width as f32;
        self.gear_rect = None;
        if self.toolbar {
            let top = self.toolbar_h;
            let text_y = (top - ch) / 2.0;
            let bar_bg = mix(pal.bg, pal.fg, 0.06);
            bgs.push(Instance::rect(0.0, 0.0, w, top, bar_bg));
            bgs.push(Instance::rect(0.0, top - thick, w, thick, mix(pal.bg, pal.fg, 0.25)));
            let mut tx = pad;
            for (mi, (title, _)) in MENUS.iter().enumerate() {
                let tw = title.chars().count() as f32 * cw + cw;
                if tx + tw > w - pad {
                    break;
                }
                let active = self.open_menu == Some(mi);
                let hot = self.hover_title == Some(mi);
                if active || hot {
                    bgs.push(Instance::rect(
                        tx,
                        0.0,
                        tw,
                        top - thick,
                        mix(pal.bg, pal.fg, if active { 0.18 } else { 0.10 }),
                    ));
                }
                self.push_text(&mut fgs, title, tx + cw * 0.5, text_y, pal.fg);
                title_rects.push((tx, tw));
                tx += tw + cw * 0.5;
            }
        }
        self.title_rects = title_rects;
        if self.toolbar {
            self.push_gear(&mut bgs, &mut fgs, pal.bg, pal.fg, w, pad, thick);
        }
        if self.tab_h() > 0.0 {
            self.push_tab_strip(&mut bgs, &mut fgs, pal.bg, pal.fg, w, pad, thick);
        }
        let sel = term.selection_bounds();
        let cursor_shown = term.cursor_visible && term.display_offset == 0;
        let cursor_vis = cursor_shown && (!view.focused || !term.cursor_blink || view.blink_on);
        let block = view.focused && term.cursor_shape == CursorShape::Block;
        for r in 0..term.rows() {
            let y = pad + self.top_inset() + r as f32 * ch;
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
                let selected = span.map_or(false, |(s, e)| c >= s && c <= e);
                let at_cursor = cursor_vis && r == term.cursor.row && c == term.cursor.col;
                if cell == Cell::BLANK && !selected && !at_cursor {
                    continue;
                }
                let is_spacer = cell.flags & WIDE_SPACER != 0;
                if is_spacer && c > 0 && !at_cursor {
                    continue;
                }
                let wide = !is_spacer
                    && row.get(c + 1).map_or(false, |n| n.flags & WIDE_SPACER != 0);
                let w = if wide { cw * 2.0 } else { cw };
                let x = pad + c as f32 * cw;
                let block_cursor = at_cursor && block;
                let mut fg = pal.rgb(cell.fg, pal.fg);
                let mut bg = pal.rgb(cell.bg, pal.bg);
                if fg == SENTINEL {
                    fg = self.about_accent();
                }
                if bg == SENTINEL {
                    bg = self.about_accent();
                }
                if fg == ART_INK {
                    fg = self.art_ink();
                }
                if bg == ART_INK {
                    bg = self.art_ink();
                }
                if cell.flags & flags::INVERSE != 0 {
                    std::mem::swap(&mut fg, &mut bg);
                }
                if block_cursor {
                    fg = bg;
                    bg = pal.cursor;
                } else if selected {
                    bg = pal.selection;
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
                    if let Some(g) = self.glyph(cell.ch, bold, italic) {
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
                if cell.flags & flags::UNDERLINE != 0 {
                    fgs.push(Instance::rect(x, y + ch - thick * 2.0, w, thick, fg));
                }
                if cell.flags & flags::STRIKE != 0 {
                    fgs.push(Instance::rect(x, y + (ch / 2.0).floor(), w, thick, fg));
                }
                if at_cursor && !block_cursor {
                    if !view.focused {
                        fgs.push(Instance::rect(x, y, cw, thick, pal.cursor));
                        fgs.push(Instance::rect(x, y + ch - thick, cw, thick, pal.cursor));
                        fgs.push(Instance::rect(x, y, thick, ch, pal.cursor));
                        fgs.push(Instance::rect(x + cw - thick, y, thick, ch, pal.cursor));
                    } else if term.cursor_shape == CursorShape::Underline {
                        fgs.push(Instance::rect(x, y + ch - thick * 2.0, cw, thick * 2.0, pal.cursor));
                    } else {
                        fgs.push(Instance::rect(x, y, thick * 2.0, ch, pal.cursor));
                    }
                }
            }
        }
        if let Some(m) = self.open_menu {
            self.push_menu_panel(&mut fgs, m, pal.bg, pal.fg, thick);
        }
        if self.settings.is_some() {
            self.push_settings(&mut fgs, pal.bg, pal.fg, thick);
        }
        self.bg_inst = bgs;
        self.fg_inst = fgs;
    }
    fn push_gear(
        &mut self,
        bgs: &mut Vec<Instance>,
        fgs: &mut Vec<Instance>,
        base: [u8; 3],
        fg: [u8; 3],
        w: f32,
        pad: f32,
        thick: f32,
    ) {
        let gw = (30.0 * self.scale).round();
        let gx = w - pad - gw;
        self.gear_rect = Some((gx, gw));
        if self.hover_gear {
            bgs.push(Instance::rect(gx, 0.0, gw, self.toolbar_h - thick, mix(base, fg, 0.10)));
        }
        let icon = (12.0 * self.scale).round().max(8.0);
        let cx = gx + gw / 2.0;
        let cy = (self.toolbar_h - thick) / 2.0;
        let col = mix(base, fg, 0.75);
        let rows = [cy - icon / 2.0, cy, cy + icon / 2.0];
        let knobs = [cx + icon * 0.18, cx - icon * 0.22, cx + icon * 0.10];
        let ks = (thick * 3.0).max(3.0);
        for i in 0..3 {
            fgs.push(Instance::rect(cx - icon / 2.0, rows[i] - thick / 2.0, icon, thick, col));
            fgs.push(Instance::rect(knobs[i] - ks / 2.0, rows[i] - ks / 2.0, ks, ks, col));
        }
    }
    fn push_tab_strip(
        &mut self,
        bgs: &mut Vec<Instance>,
        fgs: &mut Vec<Instance>,
        base: [u8; 3],
        fg: [u8; 3],
        w: f32,
        pad: f32,
        thick: f32,
    ) {
        let (cw, ch) = (self.cell_w, self.cell_h);
        let ty = self.toolbar_h;
        let th = self.tab_h();
        bgs.push(Instance::rect(0.0, ty, w, th, mix(base, fg, 0.03)));
        bgs.push(Instance::rect(0.0, ty + th - thick, w, thick, mix(base, fg, 0.18)));
        self.tab_rects.clear();
        self.close_rects.clear();
        self.new_tab_rect = None;
        self.tab_prev_rect = None;
        self.tab_next_rect = None;
        let widths = self.tab_widths();
        let n = widths.len();
        let nw = 2.0 * cw;
        let plus_x = w - pad - nw;
        let arrow_w = 2.0 * cw;
        let next_x = plus_x - cw * 0.25 - arrow_w;
        let prev_x = next_x - arrow_w;
        let limit_full = plus_x - cw * 0.5;
        let limit_arrows = prev_x - cw * 0.5;
        let fit = |start: usize, limit: f32| -> usize {
            let mut x = pad;
            let mut k = 0usize;
            while start + k < n {
                if x + widths[start + k] > limit {
                    break;
                }
                x += widths[start + k];
                k += 1;
            }
            k
        };
        let total = |a: usize| widths[a..].iter().sum::<f32>();
        let mut scroll = self.tab_scroll.min(n.saturating_sub(1));
        while scroll > 0 && total(scroll - 1) <= limit_full - pad {
            scroll -= 1;
        }
        let mut k = fit(scroll, limit_full);
        let arrows = scroll > 0 || scroll + k < n;
        if arrows {
            k = fit(scroll, limit_arrows);
        }
        self.tab_scroll = scroll;
        let tabs = self.tabs.clone();
        let active = self.active_tab;
        let mut tx = pad;
        for (i, (id, label)) in tabs.iter().enumerate().skip(scroll) {
            if i >= scroll + k {
                break;
            }
            let mut shown: String = label.chars().take(18).collect();
            if label.chars().count() > 18 {
                shown = format!("{}..", shown.chars().take(16).collect::<String>());
            }
            let tab_w = widths[i];
            let active_tab = *id == active;
            let hover = self.hover_tab == Some(*id);
            if active_tab {
                bgs.push(Instance::rect(tx, ty, tab_w, th - thick, mix(base, fg, 0.10)));
            } else if hover {
                bgs.push(Instance::rect(tx, ty, tab_w, th - thick, mix(base, fg, 0.06)));
            }
            let text_col = if active_tab { fg } else { mix(base, fg, 0.55) };
            self.push_text(fgs, &shown, tx + cw * 0.75, ty + (th - thick - ch) / 2.0, text_col);
            self.tab_rects.push((tx, tab_w, *id));
            let cxx = tx + tab_w - cw * 1.5;
            let ccol =
                if self.hover_close == Some(*id) { [0xd8, 0x3a, 0x2a] } else { mix(base, fg, 0.45) };
            let s = (cw * 1.2).round().max(thick * 3.0);
            x_cross(fgs, cxx, ty + (th - thick) / 2.0, s, thick, ccol);
            self.close_rects.push((cxx - cw * 0.75, cw * 3.0, *id));
            tx += tab_w;
        }
        let text_y = ty + (th - thick - ch) / 2.0;
        if arrows {
            let prev_col = if scroll == 0 {
                mix(base, fg, 0.25)
            } else if self.hover_prev {
                fg
            } else {
                mix(base, fg, 0.7)
            };
            let next_col = if scroll + k >= n {
                mix(base, fg, 0.25)
            } else if self.hover_next {
                fg
            } else {
                mix(base, fg, 0.7)
            };
            self.push_text(fgs, "\u{2039}", prev_x + (arrow_w - cw) / 2.0, text_y, prev_col);
            self.push_text(fgs, "\u{203a}", next_x + (arrow_w - cw) / 2.0, text_y, next_col);
            self.tab_prev_rect = Some((prev_x, arrow_w));
            self.tab_next_rect = Some((next_x, arrow_w));
        }
        let nx = plus_x;
        if self.hover_new {
            bgs.push(Instance::rect(
                nx - cw * 0.25,
                ty + thick,
                nw + cw * 0.5,
                th - 2.0 * thick,
                mix(base, fg, 0.08),
            ));
        }
        let cy = ty + (th - thick) / 2.0;
        let pl = cw * 0.9;
        let pcol = mix(base, fg, 0.7);
        fgs.push(Instance::rect(nx + nw / 2.0 - pl / 2.0, cy - thick / 2.0, pl, thick, pcol));
        fgs.push(Instance::rect(nx + nw / 2.0 - thick / 2.0, cy - pl / 2.0, thick, pl, pcol));
        self.new_tab_rect = Some((nx - cw * 0.25, nw + cw * 0.5));
    }
    fn push_menu_panel(
        &mut self,
        fgs: &mut Vec<Instance>,
        m: usize,
        base: [u8; 3],
        fg: [u8; 3],
        thick: f32,
    ) {
        let Some(g) = self.panel_geom(m) else { return };
        let (cw, ch) = (self.cell_w, self.cell_h);
        let border = mix(base, fg, 0.35);
        fgs.push(Instance::rect(g.x, g.y, g.w, g.h, mix(base, fg, 0.07)));
        fgs.push(Instance::rect(g.x, g.y, g.w, thick, border));
        fgs.push(Instance::rect(g.x, g.y + g.h - thick, g.w, thick, border));
        fgs.push(Instance::rect(g.x, g.y, thick, g.h, border));
        fgs.push(Instance::rect(g.x + g.w - thick, g.y, thick, g.h, border));
        let mut ry = g.y + ch / 2.0;
        for (i, it) in MENUS[m].1.iter().enumerate() {
            if it.action.is_none() {
                fgs.push(Instance::rect(
                    g.x + cw,
                    ry + g.sep_h / 2.0 - thick / 2.0,
                    g.w - 2.0 * cw,
                    thick,
                    mix(base, fg, 0.22),
                ));
                ry += g.sep_h;
                continue;
            }
            if self.hover_item == Some(i) {
                fgs.push(Instance::rect(
                    g.x + thick,
                    ry,
                    g.w - 2.0 * thick,
                    g.item_h,
                    mix(base, fg, 0.16),
                ));
            }
            let ty = ry + (g.item_h - ch) / 2.0;
            self.push_text(fgs, it.label, g.x + cw, ty, fg);
            if let Some(s) = it.shortcut {
                let sw = s.chars().count() as f32 * cw;
                self.push_text(fgs, s, g.x + g.w - cw - sw, ty, mix(base, fg, 0.55));
            }
            ry += g.item_h;
        }
    }
    fn push_settings(&mut self, fgs: &mut Vec<Instance>, base: [u8; 3], fg: [u8; 3], thick: f32) {
        let Some(g) = self.settings_geom() else { return };
        let Some(st) = self.settings.take() else { return };
        let (cw, ch) = (self.cell_w, self.cell_h);
        let border = mix(base, fg, 0.35);
        fgs.push(Instance::rect(g.x, g.y, g.w, g.h, mix(base, fg, 0.07)));
        fgs.push(Instance::rect(g.x, g.y, g.w, thick, border));
        fgs.push(Instance::rect(g.x, g.y + g.h - thick, g.w, thick, border));
        fgs.push(Instance::rect(g.x, g.y, thick, g.h, border));
        fgs.push(Instance::rect(g.x + g.w - thick, g.y, thick, g.h, border));
        self.push_text(fgs, "Preferências", g.x + cw * 1.5, g.y + ch * 0.55, fg);
        self.push_text(
            fgs,
            "salva em config.toml e aplica na hora; Esc cancela",
            g.x + cw * 1.5,
            g.y + ch * 1.75,
            mix(base, fg, 0.5),
        );
        let (sx, sy, sw, sh) = self.settings_seg(&g);
        let (bg1, tx1) = if st.simple {
            (mix(base, fg, 0.22), base)
        } else {
            (mix(base, fg, 0.06), mix(base, fg, 0.55))
        };
        let (bg2, tx2) = if st.simple {
            (mix(base, fg, 0.06), mix(base, fg, 0.55))
        } else {
            (mix(base, fg, 0.22), base)
        };
        fgs.push(Instance::rect(sx, sy, sw, sh, bg1));
        self.push_text(fgs, "Simples", sx + cw * 0.75, sy + (sh - ch) / 2.0, tx1);
        fgs.push(Instance::rect(sx + sw, sy, sw, sh, bg2));
        self.push_text(fgs, "Avançado", sx + sw + cw * 0.25, sy + (sh - ch) / 2.0, tx2);
        let rows = self.settings_layout(&g, st.simple, st.scroll).0;
        let card_pad = (ch * 0.5).round();
        let mut i = 0;
        while i < rows.len() {
            let sec = rows[i].sec;
            let mut j = i;
            while j < rows.len() && rows[j].sec == sec {
                j += 1;
            }
            let y0 = (rows[i].y - card_pad).max(g.rows_top);
            let y1 = (rows[j - 1].y + rows[j - 1].h + card_pad).min(g.rows_bottom);
            if y1 > y0 {
                let ccol = mix(base, fg, 0.12);
                let cx0 = g.x + cw * 0.75;
                let cwd = g.w - cw * 1.5;
                fgs.push(Instance::rect(cx0, y0, cwd, y1 - y0, mix(base, fg, 0.035)));
                fgs.push(Instance::rect(cx0, y0, cwd, thick, ccol));
                fgs.push(Instance::rect(cx0, y1 - thick, cwd, thick, ccol));
                fgs.push(Instance::rect(cx0, y0, thick, y1 - y0, ccol));
                fgs.push(Instance::rect(cx0 + cwd - thick, y0, thick, y1 - y0, ccol));
                let sec_y = rows[i].y + ch * 0.45;
                if sec_y >= g.rows_top && sec_y + ch <= g.rows_bottom {
                    self.push_text(fgs, SECTIONS[sec].0, g.x + cw * 1.75, sec_y, mix(base, fg, 0.6));
                }
            }
            i = j;
        }
        let box_w = (22.0 * cw).min((g.w - cw * 4.0).max(6.0 * cw));
        let in_rows = |y: f32, h: f32| y >= g.rows_top && y + h <= g.rows_bottom;
        for r in &rows {
            let Some(fi) = r.fld else { continue };
            let spec = &FIELDS[fi];
            let focused = st.focus == Some(fi);
            let label_y = r.y + ch * 0.35;
            let hint_y = r.y + ch * 1.45;
            if in_rows(label_y, ch) {
                self.push_text(fgs, spec.label, g.x + cw * 1.75, label_y, fg);
            }
            if in_rows(hint_y, ch) {
                self.push_text(fgs, spec.hint, g.x + cw * 1.75, hint_y, mix(base, fg, 0.45));
            }
            let box_x = g.x + g.w - cw * 1.75 - box_w;
            let box_y = label_y - thick;
            let box_h = ch + 2.0 * thick;
            let bcol = if focused { mix(base, fg, 0.55) } else { mix(base, fg, 0.28) };
            match (&st.draft[fi], spec.kind) {
                (FieldValue::Text(t), FieldKind::Color) => {
                    if !in_rows(box_y, box_h) {
                        continue;
                    }
                    let col = crate::colors::parse_hex(t).unwrap_or_else(|| mix(base, fg, 0.10));
                    fgs.push(Instance::rect(box_x, box_y, box_w, box_h, col));
                    if focused {
                        fgs.push(Instance::rect(box_x, box_y, box_w, thick, bcol));
                        fgs.push(Instance::rect(box_x, box_y + box_h - thick, box_w, thick, bcol));
                        fgs.push(Instance::rect(box_x, box_y, thick, box_h, bcol));
                        fgs.push(Instance::rect(box_x + box_w - thick, box_y, thick, box_h, bcol));
                    }
                    let tcol = if luma(col) > 140 { [0, 0, 0] } else { [255, 255, 255] };
                    let shown: String = t.chars().take((box_w / cw) as usize).collect();
                    self.push_text(
                        fgs,
                        &shown,
                        box_x + (box_w - shown.chars().count() as f32 * cw) / 2.0,
                        r.y + ch * 0.35,
                        tcol,
                    );
                }
                (FieldValue::Text(t), _) => {
                    if !in_rows(box_y, box_h) {
                        continue;
                    }
                    fgs.push(Instance::rect(box_x, box_y, box_w, box_h, mix(base, fg, 0.03)));
                    fgs.push(Instance::rect(box_x, box_y, box_w, thick, bcol));
                    fgs.push(Instance::rect(box_x, box_y + box_h - thick, box_w, thick, bcol));
                    fgs.push(Instance::rect(box_x, box_y, thick, box_h, bcol));
                    fgs.push(Instance::rect(box_x + box_w - thick, box_y, thick, box_h, bcol));
                    let max = ((box_w - 2.0 * cw) / cw).max(0.0) as usize;
                    let shown: String = t.chars().take(max).collect();
                    self.push_text(fgs, &shown, box_x + cw * 0.5, r.y + ch * 0.35, fg);
                    if focused {
                        let cx = box_x + cw * 0.5 + shown.chars().count() as f32 * cw;
                        fgs.push(Instance::rect(cx, r.y + ch * 0.35 + thick, thick, ch - 2.0 * thick, fg));
                    }
                }
                (FieldValue::Toggle(v), _) => {
                    if !in_rows(label_y, ch) {
                        continue;
                    }
                    let pill_w = 4.0 * cw;
                    let pill_x = box_x;
                    let pill_y = label_y;
                    let target = if *v { 1.0f32 } else { 0.0 };
                    let mut pos = target;
                    if let Some(&(_, t0, from)) = st.anims.iter().find(|a| a.0 == fi) {
                        let p = ((Instant::now() - t0).as_secs_f32() / TOGGLE_ANIM).clamp(0.0, 1.0);
                        let e = p * p * (3.0 - 2.0 * p);
                        pos = from + (target - from) * e;
                    }
                    let pill_bg = mix(mix(base, fg, 0.12), mix(base, fg, 0.60), pos);
                    let rim = mix(base, fg, 0.32);
                    let knob_col = mix(mix(base, fg, 0.55), base, pos);
                    fgs.push(Instance::rect(pill_x, pill_y, pill_w, ch, pill_bg));
                    fgs.push(Instance::rect(pill_x, pill_y, pill_w, thick, rim));
                    fgs.push(Instance::rect(pill_x, pill_y + ch - thick, pill_w, thick, rim));
                    fgs.push(Instance::rect(pill_x, pill_y, thick, ch, rim));
                    fgs.push(Instance::rect(pill_x + pill_w - thick, pill_y, thick, ch, rim));
                    let knob = (ch - 2.0 * thick).max(2.0);
                    let knob_x = pill_x + thick + pos * (pill_w - 2.0 * thick - knob).max(0.0);
                    fgs.push(Instance::rect(knob_x, pill_y + thick, knob, knob, knob_col));
                }
                (FieldValue::Choice(ci), _) => {
                    if !in_rows(box_y, box_h) {
                        continue;
                    }
                    fgs.push(Instance::rect(box_x, box_y, box_w, box_h, mix(base, fg, 0.03)));
                    fgs.push(Instance::rect(box_x, box_y, box_w, thick, bcol));
                    fgs.push(Instance::rect(box_x, box_y + box_h - thick, box_w, thick, bcol));
                    fgs.push(Instance::rect(box_x, box_y, thick, box_h, bcol));
                    fgs.push(Instance::rect(box_x + box_w - thick, box_y, thick, box_h, bcol));
                    let FieldKind::Choice(opts) = FIELDS[fi].kind else { unreachable!() };
                    self.push_text(fgs, opts[*ci], box_x + cw * 0.5, r.y + ch * 0.35, fg);
                }
            }
        }
        let bw = 10.0 * cw;
        let bh = ch * 1.8;
        let by = g.y + g.h - ch * 0.7 - bh;
        let save_x = g.x + g.w - cw * 1.5 - bw;
        let cancel_x = save_x - cw * 0.75 - bw;
        fgs.push(Instance::rect(save_x, by, bw, bh, mix(base, fg, 0.22)));
        self.push_text(fgs, "Salvar", save_x + cw, by + (bh - ch) / 2.0, base);
        fgs.push(Instance::rect(cancel_x, by, bw, bh, mix(base, fg, 0.08)));
        self.push_text(fgs, "Cancelar", cancel_x + cw, by + (bh - ch) / 2.0, mix(base, fg, 0.55));
        let has_picker = st.picker.is_some();
        self.settings = Some(st);
        if has_picker {
            self.push_picker(fgs, base, fg, thick);
        }
    }
    fn push_picker(&mut self, fgs: &mut Vec<Instance>, base: [u8; 3], fg: [u8; 3], thick: f32) {
        let Some(g) = self.picker_geom() else { return };
        let Some(pk) = self.settings.as_ref().and_then(|s| s.picker) else { return };
        let (cw, ch) = (self.cell_w, self.cell_h);
        let border = mix(base, fg, 0.45);
        fgs.push(Instance::rect(g.x, g.y, g.w, g.h, mix(base, fg, 0.09)));
        fgs.push(Instance::rect(g.x, g.y, g.w, thick, border));
        fgs.push(Instance::rect(g.x, g.y + g.h - thick, g.w, thick, border));
        fgs.push(Instance::rect(g.x, g.y, thick, g.h, border));
        fgs.push(Instance::rect(g.x + g.w - thick, g.y, thick, g.h, border));
        self.push_text(fgs, "Escolher cor", g.x + cw, g.y + ch * 0.55, fg);
        let new = hsv_to_rgb(pk.h, pk.s, pk.v);
        let (sx, sy, sw, sh) = g.sv;
        let (cols, rown) = (24usize, 12usize);
        for r in 0..rown {
            for c in 0..cols {
                let col = hsv_to_rgb(pk.h, c as f32 / cols as f32, 1.0 - (r as f32 + 0.5) / rown as f32);
                fgs.push(Instance::rect(
                    sx + c as f32 * sw / cols as f32,
                    sy + r as f32 * sh / rown as f32,
                    (sw / cols as f32).ceil() + 1.0,
                    (sh / rown as f32).ceil() + 1.0,
                    col,
                ));
            }
        }
        let mx = sx + pk.s * sw;
        let my = sy + (1.0 - pk.v) * sh;
        let mk = (ch * 0.9).round().max(thick * 4.0);
        square_outline(fgs, mx - mk / 2.0 - thick, my - mk / 2.0 - thick, mk + 2.0 * thick, thick, [0, 0, 0]);
        square_outline(fgs, mx - mk / 2.0, my - mk / 2.0, mk, thick, [255, 255, 255]);
        let (hx, hy, hw, hh) = g.hue;
        for c in 0..24usize {
            let col = hsv_to_rgb(c as f32 * 15.0, 1.0, 1.0);
            fgs.push(Instance::rect(
                hx + c as f32 * hw / 24.0,
                hy,
                (hw / 24.0).ceil() + 1.0,
                hh,
                col,
            ));
        }
        let hxx = hx + pk.h / 360.0 * hw;
        square_outline(fgs, hxx - mk / 2.0 - thick, hy + hh / 2.0 - mk / 2.0 - thick, mk + 2.0 * thick, thick, [0, 0, 0]);
        square_outline(fgs, hxx - mk / 2.0, hy + hh / 2.0 - mk / 2.0, mk, thick, [255, 255, 255]);
        let (px, py, pw2, ph2) = g.preview;
        let half = (pw2 - thick) / 2.0;
        let cur = match self.settings.as_ref().and_then(|s| s.draft.get(pk.field)) {
            Some(FieldValue::Text(t)) => crate::colors::parse_hex(t).unwrap_or(base),
            _ => base,
        };
        fgs.push(Instance::rect(px, py, half, ph2, cur));
        fgs.push(Instance::rect(px + half + thick, py, half, ph2, new));
        self.push_text(
            fgs,
            "atual",
            px + (half - 5.0 * cw) / 2.0,
            py + (ph2 - ch) / 2.0,
            if luma(cur) > 140 { [0, 0, 0] } else { [255, 255, 255] },
        );
        self.push_text(
            fgs,
            "novo",
            px + half + thick + (half - 4.0 * cw) / 2.0,
            py + (ph2 - ch) / 2.0,
            if luma(new) > 140 { [0, 0, 0] } else { [255, 255, 255] },
        );
        self.push_text(fgs, &format!("Hex {}", hex_str(new)), g.x + cw, g.y + ch * 13.55, fg);
        let rgb = format!("RGB {}, {}, {}", new[0], new[1], new[2]);
        self.push_text(fgs, &rgb, g.x + cw * 11.0, g.y + ch * 13.55, mix(base, fg, 0.6));
        let (ox, oy, ow, oh) = g.ok;
        fgs.push(Instance::rect(ox, oy, ow, oh, mix(base, fg, 0.25)));
        self.push_text(fgs, "Confirmar", ox + cw * 0.5, oy + (oh - ch) / 2.0, base);
        let (cx, cy, cw2, ch2) = g.cancel;
        fgs.push(Instance::rect(cx, cy, cw2, ch2, mix(base, fg, 0.10)));
        self.push_text(fgs, "Cancelar", cx + cw, cy + (ch2 - ch) / 2.0, mix(base, fg, 0.55));
    }
    pub fn draw(&mut self) {
        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            Err(_) => return,
        };
        let tex_view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let n_bg = self.bg_inst.len();
        let n = n_bg + self.fg_inst.len();
        if n > self.inst_cap {
            self.inst_cap = n.next_power_of_two();
            self.inst_buf = Self::make_inst_buf(&self.device, self.inst_cap);
        }
        let isz = std::mem::size_of::<Instance>() as u64;
        self.queue.write_buffer(&self.inst_buf, 0, bytemuck::cast_slice(&self.bg_inst));
        self.queue.write_buffer(&self.inst_buf, n_bg as u64 * isz, bytemuck::cast_slice(&self.fg_inst));
        let bg = self.effective_bg;
        let clear = wgpu::Color {
            r: bg[0] as f64 / 255.0,
            g: bg[1] as f64 / 255.0,
            b: bg[2] as f64 / 255.0,
            a: 1.0,
        };
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &tex_view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            if n > 0 {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_vertex_buffer(0, self.inst_buf.slice(..));
                pass.draw(0..4, 0..n as u32);
            }
        }
        self.queue.submit(Some(enc.finish()));
        frame.present();
    }
}
fn measure_cell_width(fs: &mut FontSystem, family: &str, size: f32, line_h: f32) -> f32 {
    let mut buf = Buffer::new(fs, Metrics::new(size, line_h));
    let attrs = Attrs::new().family(Family::Name(family));
    buf.set_text(fs, "M", attrs, Shaping::Advanced);
    buf.layout_runs()
        .next()
        .and_then(|r| r.glyphs.first().map(|g| g.w))
        .unwrap_or(size * 0.6)
}
