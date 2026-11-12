use std::collections::HashMap;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use cosmic_text::{
    Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Style, SwashCache, SwashContent, Weight,
};
use wgpu::util::DeviceExt;
use winit::window::Window;

use crate::cell::{flags, Cell};
use crate::colors::Palette;
use crate::config::Config;
use crate::term::{CursorShape, Term, WIDE_SPACER};

const ATLAS_SIZE: u32 = 2048;
const WHITE_UV: [f32; 2] = [1.0 / ATLAS_SIZE as f32, 1.0 / ATLAS_SIZE as f32];

/// Estado de UI que influencia o desenho (não faz parte do terminal em si).
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

#[derive(Clone, Copy)]
struct GlyphInfo {
    uv_pos: [f32; 2],
    uv_size: [f32; 2],
    off: [f32; 2],
    size: [f32; 2],
    is_color: bool,
}

/// Atlas R8 com empacotamento em "prateleiras". O bloco 2x2 em (0,0) é branco.
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

    /// Esvazia o empacotamento (zoom ou nova configuração). O bloco branco é mantido.
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

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// Recalcula tamanho de célula a partir da fonte/zoom atuais e esvazia os caches.
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

    /// Muda o zoom da fonte; o chamador precisa refazer o layout da grade depois.
    pub fn set_zoom(&mut self, z: f32) {
        self.zoom = (z.clamp(0.5, 4.0) * 20.0).round() / 20.0;
        self.recompute_metrics();
    }

    /// Aplica fonte, padding e cores de uma configuração (nova ou recarregada).
    /// O chamador precisa refazer o layout da grade depois.
    pub fn apply_config(&mut self, cfg: &Config) {
        self.font_family = cfg.font.family.clone();
        self.base_font_size = cfg.font.size;
        self.line_height = cfg.font.line_height;
        self.window_padding = cfg.window.padding;
        self.pad = (self.window_padding * self.scale).round();
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

    /// (colunas, linhas) que cabem na janela atual, descontando a margem.
    pub fn grid_size(&self) -> (usize, usize) {
        let w = self.config.width as f32 - 2.0 * self.pad;
        let h = self.config.height as f32 - 2.0 * self.pad;
        let cols = (w / self.cell_w) as usize;
        let rows = (h / self.cell_h) as usize;
        (cols.max(1), rows.max(1))
    }

    /// Célula (linha, coluna) sob um ponto em pixels físicos, sempre dentro da grade.
    pub fn cell_at(&self, x: f64, y: f64) -> (usize, usize) {
        let (cols, rows) = self.grid_size();
        let c = ((x as f32 - self.pad) / self.cell_w).floor().max(0.0) as usize;
        let r = ((y as f32 - self.pad) / self.cell_h).floor().max(0.0) as usize;
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
        let attrs = Attrs::new()
            .family(Family::Name(&family))
            .weight(if bold { Weight::BOLD } else { Weight::NORMAL })
            .style(if italic { Style::Italic } else { Style::Normal });

        let mut buf = Buffer::new(&mut self.font_system, Metrics::new(self.font_size, self.cell_h));
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

    /// Monta as instâncias a partir do terminal. Roda com o lock do Term preso,
    /// então não toca na GPU: o desenho de verdade fica em `draw`.
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

        let sel = term.selection_bounds();
        let cursor_shown = term.cursor_visible && term.display_offset == 0;
        let cursor_vis = cursor_shown && (!view.focused || !term.cursor_blink || view.blink_on);
        let block = view.focused && term.cursor_shape == CursorShape::Block;

        for r in 0..term.rows() {
            let y = pad + r as f32 * ch;
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

        self.bg_inst = bgs;
        self.fg_inst = fgs;
    }

    /// Envia as instâncias para a GPU e apresenta o frame. Roda SEM o lock do Term.
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
