use std::borrow::Cow;
use std::collections::HashMap;

use crate::graphics::Image;

const KEEP_FRAMES: u64 = 600;
const BUDGET: usize = 512 << 20;

struct GpuImage {
    bind_group: wgpu::BindGroup,
    _texture: wgpu::Texture,
    used: u64,
    bytes: usize,
}

pub(super) struct ImageCache {
    textures: HashMap<u64, GpuImage>,
    sampler: wgpu::Sampler,
    frame: u64,
    max_dim: u32,
    pub uploads: u64,
}

impl ImageCache {
    pub fn new(device: &wgpu::Device) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("images"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let max_dim = device.limits().max_texture_dimension_2d;
        Self { textures: HashMap::new(), sampler, frame: 0, max_dim, uploads: 0 }
    }
    pub fn begin_frame(&mut self) {
        self.frame += 1;
    }
    pub fn ensure(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        globals: &wgpu::Buffer,
        img: &Image,
    ) {
        if let Some(t) = self.textures.get_mut(&img.uid) {
            t.used = self.frame;
            return;
        }
        let (w, h, data) = shrink(img, self.max_dim);
        let size = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("image"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &data,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
            size,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("image"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        self.uploads += 1;
        let bytes = data.len();
        self.textures.insert(img.uid, GpuImage { bind_group, _texture: texture, used: self.frame, bytes });
    }
    pub fn bind_group(&self, uid: u64) -> Option<&wgpu::BindGroup> {
        self.textures.get(&uid).map(|t| &t.bind_group)
    }
    pub fn end_frame(&mut self) {
        let frame = self.frame;
        self.textures.retain(|_, t| t.used + KEEP_FRAMES >= frame);
        let mut total: usize = self.textures.values().map(|t| t.bytes).sum();
        while total > BUDGET {
            let Some((&uid, t)) =
                self.textures.iter().filter(|(_, t)| t.used < frame).min_by_key(|(_, t)| t.used)
            else {
                break;
            };
            total -= t.bytes;
            self.textures.remove(&uid);
        }
    }
}

fn shrink(img: &Image, max: u32) -> (u32, u32, Cow<'_, [u8]>) {
    let (w, h) = (img.width, img.height);
    if w <= max && h <= max {
        return (w, h, Cow::Borrowed(&img.rgba));
    }
    let s = max as f64 / w.max(h) as f64;
    let nw = ((w as f64 * s) as u32).clamp(1, max);
    let nh = ((h as f64 * s) as u32).clamp(1, max);
    let mut out = Vec::with_capacity(nw as usize * nh as usize * 4);
    for y in 0..nh {
        let sy = (y as u64 * h as u64 / nh as u64) as usize;
        for x in 0..nw {
            let sx = (x as u64 * w as u64 / nw as u64) as usize;
            let i = (sy * w as usize + sx) * 4;
            out.extend_from_slice(&img.rgba[i..i + 4]);
        }
    }
    (nw, nh, Cow::Owned(out))
}
