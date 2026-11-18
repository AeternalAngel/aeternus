struct Globals {
    screen: vec2<f32>,
    text: vec2<f32>,
    mode: vec4<f32>,
};

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn out_color(c: vec4<f32>) -> vec4<f32> {
    if g.mode.x > 0.5 {
        return vec4<f32>(to_linear(c.rgb), c.a);
    }
    return c;
}

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct VsIn {
    @builtin(vertex_index) vi: u32,
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) uv_pos: vec2<f32>,
    @location(3) uv_size: vec2<f32>,
    @location(4) color: vec4<f32>,
    @location(5) color_glyph: f32,
    @location(6) under: vec3<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) color_glyph: f32,
    @location(3) under: vec3<f32>,
};

const LUMA: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);

@vertex
fn vs_main(inp: VsIn) -> VsOut {
    let corner = vec2<f32>(f32(inp.vi & 1u), f32((inp.vi >> 1u) & 1u));
    let p = inp.pos + corner * inp.size;
    let ndc = vec2<f32>(p.x / g.screen.x * 2.0 - 1.0, 1.0 - p.y / g.screen.y * 2.0);
    var o: VsOut;
    o.clip = vec4<f32>(ndc, 0.0, 1.0);
    o.uv = inp.uv_pos + corner * inp.uv_size;
    o.color = out_color(inp.color);
    o.color_glyph = inp.color_glyph;
    o.under = out_color(vec4<f32>(inp.under, 1.0)).rgb;
    return o;
}

@fragment
fn fs_main(inp: VsOut) -> @location(0) vec4<f32> {
    let tex = textureSample(atlas, samp, inp.uv);
    if inp.color_glyph > 0.5 {
        let rgb = select(tex.rgb, to_linear(tex.rgb), g.mode.x > 0.5);
        return vec4<f32>(rgb, tex.a * inp.color.a);
    }
    var a = tex.r;
    if g.text.x != 1.0 || g.text.y > 0.0 {
        let w = clamp((1.0 - dot(inp.color.rgb, LUMA) + dot(inp.under, LUMA)) * 0.5, 0.0, 1.0);
        a = clamp(mix(a, pow(a, 1.0 / g.text.x), w) * (1.0 + g.text.y), 0.0, 1.0);
    }
    return vec4<f32>(inp.color.rgb, inp.color.a * a);
}

struct PolyIn {
    @location(0) pos: vec2<f32>,
    @location(1) color: vec4<f32>,
};

struct PolyOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_poly(inp: PolyIn) -> PolyOut {
    let ndc = vec2<f32>(inp.pos.x / g.screen.x * 2.0 - 1.0, 1.0 - inp.pos.y / g.screen.y * 2.0);
    var o: PolyOut;
    o.clip = vec4<f32>(ndc, 0.0, 1.0);
    o.color = out_color(inp.color);
    return o;
}

@fragment
fn fs_poly(inp: PolyOut) -> @location(0) vec4<f32> {
    return inp.color;
}
