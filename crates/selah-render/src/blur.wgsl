// Separable 1-D Gaussian blur + optional opacity composite (M3.4 / M3.5).
// Matches blur.rs CPU implementation for exact GPU/CPU parity.

struct BlurParams {
    width: u32,
    height: u32,
    dir_x: f32,
    dir_y: f32,
    radius: i32,
    opacity: f32,
    _pad0: u32,
    _pad1: u32,
    // Packed weights[0..=32] (actually up to index 24 needed) stored as vec4 arrays for std140/uniform alignment.
    weights: array<vec4<f32>, 9>,
};

@group(0) @binding(0) var<uniform> params: BlurParams;
@group(0) @binding(1) var src_tex: texture_2d<f32>;
@group(0) @binding(2) var src_sampler: sampler;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOut {
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    var out: VertexOut;
    out.position = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, y);
    return out;
}

fn weight_at(k: i32) -> f32 {
    let idx = u32(k);
    let v = params.weights[idx / 4u];
    switch idx % 4u {
        case 0u: { return v.x; }
        case 1u: { return v.y; }
        case 2u: { return v.z; }
        default: { return v.w; }
    }
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let texel = vec2<f32>(
        params.dir_x / f32(max(params.width, 1u)),
        params.dir_y / f32(max(params.height, 1u)),
    );
    var accum = textureSampleLevel(src_tex, src_sampler, clamp(in.uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0) * weight_at(0);
    for (var k: i32 = 1; k <= params.radius; k = k + 1) {
        let w = weight_at(k);
        let off = texel * f32(k);
        let c1 = textureSampleLevel(src_tex, src_sampler, clamp(in.uv + off, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
        let c2 = textureSampleLevel(src_tex, src_sampler, clamp(in.uv - off, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
        accum = accum + (c1 + c2) * w;
    }
    return clamp(accum * params.opacity, vec4<f32>(0.0), vec4<f32>(1.0));
}
